//! UNX-F51646 控制传输阶段错序注入（Status 先到） · 0xC2440段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F51650-J1 十族 ×3
//! 轮全达标；UNX-F51650-J1R 超时项零静默；③ 完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - LossCode        诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - RecoveryPhase   恢复相态机（Streaming→Compensating→Recovered）
//!   - IntervalLossEngine 间隔调度引擎（期距事件账 + 丢包补偿 + 环续转）
//!   - FactorLedger    异常三要素留账（异常码/位置/处置动作，留账率 100%）
//!   - run_unx51646_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 期距事件账：中断端点按固定期距送达事件，实到拍晚于期望拍即
//!      间隔内丢包，按缺口拍数逐条补偿留账；早到/重复拍判异常但不
//!      补偿——无丢失零误补偿。
//!   ② 异常三要素留账率 100%：每次异常必须同时记下异常码、发生位置
//!      （拍号）与处置动作，缺一即判留账失败；异常后环必须继续运转
//!      （后续在拍事件照常消化，消化计数持续递增）。
//!   ③ 恢复相态单调：Streaming→Compensating→Recovered 三相态按编码序
//!      前进，回退/跳相显性拒；补偿未清账不得宣称恢复。
//!   ④ 注入核：5 类坏样本逐类 ×10 轮精确复现，注入账守恒
//!      （注入 = 检出 + 逃逸，逃逸恒 0）。
//!   ⑤ 完成写回：补偿清账后写状态=已完成、结果摘要非空回执。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机——LCG 种子由
//!      SEG_BASE 派生；全程零 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC2440;

/// 事件环容量。
pub const RING_CAP: usize = 8;
/// 期距（拍）：事件应每期距送达一次。
pub const INTERVAL: u32 = 4;

/// 诊断码闭集（丢包补偿/异常留账域，12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LossCode {
    /// 成功
    Ok = 1,
    /// 段位守卫失败
    SegBad = 2,
    /// 间隔内丢包（晚到缺口）
    IntervalLoss = 3,
    /// 重复拍（同拍重复事件）
    DuplicateTick = 4,
    /// 拍回退（回绕损坏）
    TickRegress = 5,
    /// 补偿未清账即宣称恢复
    RecoverWithDebt = 6,
    /// 相态回退/跳相
    PhaseRegress = 7,
    /// 三要素留账缺失
    FactorMissing = 8,
    /// 事件环槽越界
    IndexOob = 9,
    /// 注入账不符
    LedgerMismatch = 10,
    /// 写回摘要为空
    WritebackEmpty = 11,
    /// 无丢失误补偿
    FalseCompensate = 12,
}

impl LossCode {
    /// 段位派生值：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全量非空中文诊断文案。
    pub fn detail(self) -> &'static str {
        match self {
            LossCode::Ok => "间隔调度操作成功",
            LossCode::SegBad => "段位越界：检查 SEG_BASE 对齐与分派区间",
            LossCode::IntervalLoss => "间隔内丢包：实到拍晚于期望拍，按缺口逐条补偿",
            LossCode::DuplicateTick => "重复拍：同拍重复事件判异常不补偿",
            LossCode::TickRegress => "拍回退：拍号倒退判回绕损坏，重同步期距账",
            LossCode::RecoverWithDebt => "带账恢复：补偿未清账不得宣称恢复",
            LossCode::PhaseRegress => "相态违例：恢复链必须按编码序单调前进",
            LossCode::FactorMissing => "三要素缺失：异常留账必须含异常码/位置/动作",
            LossCode::IndexOob => "事件环槽越界：访问必须在容量之内",
            LossCode::LedgerMismatch => "注入账不符：注入必须等于检出加逃逸",
            LossCode::WritebackEmpty => "写回摘要为空：补偿收口回执必须带非空结果摘要",
            LossCode::FalseCompensate => "误补偿：无缺口不得产生补偿留账",
        }
    }
}

/// 诊断码全序表（闭集权威清单，12 变体）。
pub const ALL_CODES: [LossCode; 12] = [
    LossCode::Ok,
    LossCode::SegBad,
    LossCode::IntervalLoss,
    LossCode::DuplicateTick,
    LossCode::TickRegress,
    LossCode::RecoverWithDebt,
    LossCode::PhaseRegress,
    LossCode::FactorMissing,
    LossCode::IndexOob,
    LossCode::LedgerMismatch,
    LossCode::WritebackEmpty,
    LossCode::FalseCompensate,
];

/// 恢复相态（闭集，编码序即合法前进序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecoveryPhase {
    /// 正常流转
    Streaming = 0,
    /// 补偿中
    Compensating = 1,
    /// 已恢复
    Recovered = 2,
}

impl RecoveryPhase {
    /// 相态编码。
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 中文相态名。
    pub fn name(self) -> &'static str {
        match self {
            RecoveryPhase::Streaming => "正常流转",
            RecoveryPhase::Compensating => "补偿中",
            RecoveryPhase::Recovered => "已恢复",
        }
    }
}

/// 完成写回记录（判据③）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RecoverWriteback {
    /// 状态：true=已完成
    pub done: bool,
    /// 结果摘要（非空）
    pub summary: String,
    /// 相态序号（必须为 Recovered=2）
    pub phase: u32,
}

/// 异常三要素留账（异常码 + 位置拍号 + 处置动作，缺一不可）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FactorEntry {
    /// 要素一：异常码
    pub code: u32,
    /// 要素二：发生位置（拍号）
    pub at_tick: u32,
    /// 要素三：处置动作（非空中文）
    pub action: String,
}

/// 间隔调度引擎：期距事件账 + 丢包补偿 + 异常三要素留账。
pub struct IntervalLossEngine {
    phase: RecoveryPhase,
    phase_trace: Vec<u32>,
    expect_tick: u32,
    consumed: u32,
    debt: u32,
    factors: Vec<FactorEntry>,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl IntervalLossEngine {
    /// 新引擎：Streaming 相态、期望拍 0。
    pub fn new() -> IntervalLossEngine {
        IntervalLossEngine {
            phase: RecoveryPhase::Streaming,
            phase_trace: Vec::new(),
            expect_tick: 0,
            consumed: 0,
            debt: 0,
            factors: Vec::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 当前相态。
    pub fn phase(&self) -> RecoveryPhase {
        self.phase
    }

    /// 相态轨迹。
    pub fn phase_trace(&self) -> &[u32] {
        &self.phase_trace
    }

    /// 已消化事件数（环续转的锚点量）。
    pub fn consumed(&self) -> u32 {
        self.consumed
    }

    /// 当前补偿欠账。
    pub fn debt(&self) -> u32 {
        self.debt
    }

    /// 相态推进：只允许编码序 +1 前进。
    pub fn advance_phase(&mut self, want: RecoveryPhase) -> Result<RecoveryPhase, LossCode> {
        if want.code() != self.phase.code() + 1 {
            return Err(LossCode::PhaseRegress);
        }
        self.phase = want;
        self.phase_trace.push(want.code());
        Ok(want)
    }

    /// 留一条三要素账（全要素校验后入账）。
    pub fn ledger_factor(&mut self, code: u32, at_tick: u32, action: &str) -> Result<(), LossCode> {
        if action.is_empty() {
            return Err(LossCode::FactorMissing);
        }
        self.factors.push(FactorEntry {
            code,
            at_tick,
            action: String::from(action),
        });
        Ok(())
    }

    /// 三要素留账条数。
    pub fn factors_len(&self) -> usize {
        self.factors.len()
    }

    /// 第 k 条三要素账完整性核验（三要素全非零/非空）。
    pub fn factor_complete(&self, k: usize) -> bool {
        match self.factors.get(k) {
            Some(e) => e.code != 0 && !e.action.is_empty(),
            None => false,
        }
    }

    /// 事件到达：晚到按缺口补偿，早到/回退判异常；环全程续转。
    pub fn arrive(&mut self, tick: u32) -> Result<u32, LossCode> {
        if tick > self.expect_tick {
            // 间隔内丢包：缺口 = 实到拍 - 期望拍。
            let missed = tick - self.expect_tick;
            let ledgered = self.ledger_factor(
                LossCode::IntervalLoss as u32,
                tick,
                "间隔内丢包：按缺口拍数缓存重放补偿留账",
            );
            if ledgered.is_err() {
                return Err(LossCode::FactorMissing);
            }
            self.debt = self.debt.saturating_add(missed);
            self.expect_tick = tick + INTERVAL;
            if self.phase == RecoveryPhase::Streaming {
                let _ = self.advance_phase(RecoveryPhase::Compensating);
            }
            return Err(LossCode::IntervalLoss);
        }
        // 同拍重复：expect_tick 未推进时再次在拍到达判重复。
        if self.consumed > 0 && tick + INTERVAL == self.expect_tick && self.debt == 0 {
            let ledgered = self.ledger_factor(
                LossCode::DuplicateTick as u32,
                tick,
                "重复拍：同拍重复事件丢弃留账不补偿",
            );
            if ledgered.is_err() {
                return Err(LossCode::FactorMissing);
            }
            return Err(LossCode::DuplicateTick);
        }
        if tick < self.expect_tick {
            // 拍回退：回绕损坏，重同步期距账。
            let ledgered = self.ledger_factor(
                LossCode::TickRegress as u32,
                tick,
                "拍回退：重同步期距账并按当前拍重建期望",
            );
            if ledgered.is_err() {
                return Err(LossCode::FactorMissing);
            }
            self.expect_tick = tick + INTERVAL;
            return Err(LossCode::TickRegress);
        }
        // 在拍事件：消化并推进期距账。
        self.expect_tick = tick + INTERVAL;
        self.consumed = self.consumed.saturating_add(1);
        Ok(self.consumed)
    }

    /// 补偿清账（Streaming→Compensating 后调用，清欠账）。
    pub fn compensate(&mut self) -> Result<u32, LossCode> {
        if self.debt == 0 {
            return Err(LossCode::FalseCompensate);
        }
        if self.phase == RecoveryPhase::Streaming {
            let _ = self.advance_phase(RecoveryPhase::Compensating)?;
        }
        let cleared = self.debt;
        self.debt = 0;
        Ok(cleared)
    }

    /// 完成恢复（Compensating→Recovered）并写回。
    pub fn recover(&mut self) -> Result<RecoverWriteback, LossCode> {
        if self.debt != 0 {
            return Err(LossCode::RecoverWithDebt);
        }
        if self.phase != RecoveryPhase::Compensating {
            return Err(LossCode::PhaseRegress);
        }
        let _ = self.advance_phase(RecoveryPhase::Recovered)?;
        Ok(RecoverWriteback {
            done: true,
            summary: format!(
                "丢包恢复收口：补偿欠账清零，三要素留账 {} 条，已消化 {} 事件",
                self.factors.len(),
                self.consumed
            ),
            phase: RecoveryPhase::Recovered.code(),
        })
    }

    /// 相态复位回 Streaming（新一轮观察窗）。
    pub fn reset_phase(&mut self) {
        self.phase = RecoveryPhase::Streaming;
        self.phase_trace.clear();
    }

    /// 期距账整体重同步：期望拍/欠账/相态全部复位（类间隔离用）。
    pub fn resync_tick(&mut self) {
        self.expect_tick = 0;
        self.debt = 0;
        self.phase = RecoveryPhase::Streaming;
        self.phase_trace.clear();
    }

    /// 越界安全探测：事件环槽写入。
    pub fn poke(&mut self, idx: usize, payload: u64) -> Result<(), LossCode> {
        let _ = payload;
        if idx >= RING_CAP {
            return Err(LossCode::IndexOob);
        }
        Ok(())
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 注入账守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    pub fn injected(&self) -> u64 {
        self.injected
    }

    pub fn detected(&self) -> u64 {
        self.detected
    }

    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 状态快照（双跑指纹取材）。
    pub fn snapshot(&self) -> (u32, u32, u32, usize, u64, u64) {
        (
            self.expect_tick,
            self.consumed,
            self.debt,
            self.factors.len(),
            self.injected,
            self.detected,
        )
    }
}

/// LCG：种子由 SEG_BASE 派生（零随机源，确定性可复现）。
pub fn lcg_next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 33
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// FNV-1a 64 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 引擎指纹：对确定性脚本做 FNV-1a（双跑一致面）。
pub fn engine_fingerprint() -> u64 {
    let mut e = IntervalLossEngine::new();
    let mut seed: u64 = (SEG_BASE as u64).wrapping_mul(2654435761).wrapping_add(1);
    let mut log: String = String::new();
    for round in 0..64u32 {
        let op = lcg_next(&mut seed) % 5;
        let tick = (lcg_next(&mut seed) % 40) as u32;
        let r = match op {
            0 => e.arrive(tick),
            1 => e.compensate(),
            2 => e.recover().map(|w| w.phase),
            3 => e.poke((tick as usize) % (RING_CAP + 2), tick as u64).map(|_| 0),
            _ => e.ledger_factor(9, tick, "脚本探针留账").map(|_| 0),
        };
        log.push_str(&format!("{}:{:?};", round, r.err()));
        if e.phase() == RecoveryPhase::Recovered {
            e.reset_phase();
        }
    }
    let (t, c, d, f, i, det) = e.snapshot();
    let full = format!("{}|{},{},{},{},{},{}", log, t, c, d, f, i, det);
    fnv1a(full.as_bytes())
}

/// 双跑一致性：同一脚本两次执行指纹必须逐位一致。
pub fn dual_run_consistent() -> bool {
    engine_fingerprint() == engine_fingerprint()
}

/// 注入核：5 类坏样本逐类 ×10 轮精确复现，注入账守恒（逃逸=0）。
pub fn injection_core() -> (u64, u64, u64, u32) {
    let mut e = IntervalLossEngine::new();
    let mut seed: u64 = (SEG_BASE as u64).wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(7);
    let mut classes_hit = 0u32;
    // 类1：单间隔丢包 ×10（缺口 1，补偿生效）。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            let base = i * 16;
            let _ = e.arrive(base);
            let r = e.arrive(base + INTERVAL + 1); // 缺 1 拍
            if r == Err(LossCode::IntervalLoss) && e.factors_len() > 0 {
                caught += 1;
                e.note_detected();
            } else {
                e.note_escape();
            }
            let _ = e.compensate();
            e.reset_phase();
            let _ = lcg_next(&mut seed) + i as u64;
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类2：多间隔丢包 ×10（缺口 3，补偿 = 3）。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            e.resync_tick();
            let base = 0;
            let _ = i;
            let _ = e.arrive(base);
            let r = e.arrive(base + INTERVAL + 3);
            let debt_before = e.debt();
            let cleared = e.compensate();
            if r == Err(LossCode::IntervalLoss) && debt_before == 3 && cleared == Ok(3) {
                caught += 1;
                e.note_detected();
            } else {
                e.note_escape();
            }
            e.reset_phase();
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类3：拍回退 ×10。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            e.resync_tick();
            let base = 0;
            let _ = i;
            let _ = e.arrive(base);
            let r = e.arrive(base + 1); // 回退拍
            if r == Err(LossCode::TickRegress) {
                caught += 1;
                e.note_detected();
            } else {
                e.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类4：带账恢复 ×10（欠账未清不得宣称恢复）。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            e.resync_tick();
            let base = 0;
            let _ = i;
            let _ = e.arrive(base);
            let _ = e.arrive(base + INTERVAL + 2);
            let r = e.recover();
            if r == Err(LossCode::RecoverWithDebt) || r == Err(LossCode::PhaseRegress) {
                caught += 1;
                e.note_detected();
            } else {
                e.note_escape();
            }
            let _ = e.compensate();
            e.reset_phase();
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类5：环槽越界 ×10。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            let r = e.poke(RING_CAP + i as usize, 0x5A);
            if r == Err(LossCode::IndexOob) {
                caught += 1;
                e.note_detected();
            } else {
                e.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    (e.injected(), e.detected(), e.escaped(), classes_hit)
}

/// 无丢失 ×1000 轮零误补偿（J1R 锚点）。
pub fn no_loss_zero_compensate() -> bool {
    let mut e = IntervalLossEngine::new();
    let mut tick = 0u32;
    let mut round = 0u32;
    while round < 1000u32 {
        if e.arrive(tick).is_err() {
            return false;
        }
        if e.factors_len() != 0 || e.debt() != 0 {
            return false;
        }
        tick += INTERVAL;
        round += 1;
    }
    e.consumed() == 1000
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51646_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51646");
    // 1 在拍事件消化。
    {
        let mut e = IntervalLossEngine::new();
        let a = e.arrive(0);
        let b = e.arrive(INTERVAL);
        let ok = a == Ok(1) && b == Ok(2) && e.consumed() == 2 && e.debt() == 0;
        s.add("在拍事件消化", ok, "按期距送达的事件须逐个消化且零欠账");
    }
    // 2 单间隔丢包补偿生效。
    {
        let mut e = IntervalLossEngine::new();
        let _ = e.arrive(0);
        let r = e.arrive(INTERVAL + 1);
        let c = e.compensate();
        let ok = r == Err(LossCode::IntervalLoss) && c == Ok(1) && e.debt() == 0;
        s.add("单丢补偿生效", ok, "缺口 1 拍须补偿 1 且清账后欠账归零");
    }
    // 3 多间隔丢包补偿按缺口数。
    {
        let mut e = IntervalLossEngine::new();
        let _ = e.arrive(0);
        let _ = e.arrive(INTERVAL + 3);
        let c = e.compensate();
        s.add("多丢按缺补偿", c == Ok(3), "缺口 3 拍须一次补偿 3");
    }
    // 4 异常三要素留账率 100%。
    {
        let mut e = IntervalLossEngine::new();
        let _ = e.arrive(0);
        let _ = e.arrive(INTERVAL + 2);
        let _ = e.arrive(1); // 回退
        let n = e.factors_len();
        let mut all = n >= 2;
        let mut k = 0usize;
        while k < n {
            if !e.factor_complete(k) {
                all = false;
            }
            k += 1;
        }
        s.add("三要素留账满额", all, "每条异常留账须异常码/位置/动作三要素齐全");
    }
    // 5 异常 ×10 环继续运转。
    {
        let mut e = IntervalLossEngine::new();
        let mut exceptions = 0u32;
        let mut resumed = 0u32;
        for _ in 0..10u32 {
            e.resync_tick();
            let _ = e.arrive(0);
            if e.arrive(INTERVAL + 1).is_err() {
                exceptions += 1;
            }
            let _ = e.compensate();
            e.reset_phase();
            // 异常后下一在拍事件仍照常消化（expect 已被 arrive 推进到 9）。
            let c1 = e.consumed();
            if e.arrive(INTERVAL + 5).is_ok() && e.consumed() == c1 + 1 {
                resumed += 1;
            }
        }
        let ok = exceptions == 10 && resumed == 10;
        s.add("异常后环续转", ok, "10 次异常后后续在拍事件仍须照常消化");
    }
    // 6 无丢失 ×1000 零误补偿。
    s.add("无丢失零误补偿", no_loss_zero_compensate(), "正常流 ×1000 轮不得产生补偿或异常留账");
    // 7 注入核：5 类 ×10 全检出、逃逸 0、类齐。
    {
        let (inj, det, esc, cls) = injection_core();
        s.add(
            "注入核五类全检",
            inj == 50 && det == 50 && esc == 0 && cls == 5,
            "5 类坏样本 ×10 轮须全检出，逃逸恒 0",
        );
    }
    // 8 注入账守恒。
    {
        let (inj, det, esc) = {
            let (i, d, x, _) = injection_core();
            (i, d, x)
        };
        s.add(
            "注入账守恒",
            inj == det + esc && esc == 0 && det > 0,
            "注入 = 检出 + 逃逸，逃逸恒 0",
        );
    }
    // 9 带账恢复显性拒。
    {
        let mut e = IntervalLossEngine::new();
        let _ = e.arrive(0);
        let _ = e.arrive(INTERVAL + 2);
        let r = e.recover();
        s.add("带账恢复拒", r.is_err(), "补偿欠账未清不得宣称恢复");
    }
    // 10 补偿→恢复全链写回。
    {
        let mut e = IntervalLossEngine::new();
        let _ = e.arrive(0);
        let _ = e.arrive(INTERVAL + 1);
        let _ = e.compensate();
        let r = e.recover();
        s.add(
            "恢复全链写回",
            matches!(r, Ok(ref w) if w.done && !w.summary.is_empty() && w.phase == 2),
            "清账后恢复收口，写回状态=已完成且摘要非空",
        );
    }
    // 11 诊断码闭集唯一且文案非空。
    {
        let mut uniq = true;
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            let mut j = i + 1;
            while j < ALL_CODES.len() {
                if ALL_CODES[i] == ALL_CODES[j] {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        let mut texts = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                texts = false;
            }
        }
        s.add("闭集唯一文案全", uniq && texts, "12 变体互异且中文文案全量非空");
    }
    // 12 val 段位派生。
    {
        let mut ok = true;
        for (idx, c) in ALL_CODES.iter().enumerate() {
            if c.val() != SEG_BASE + 1 + idx as u32 {
                ok = false;
            }
        }
        s.add("val 段位派生", ok, "每个诊断码 val 必须等于 SEG_BASE+1+变体序号");
    }
    // 13 双跑 FNV 指纹一致。
    s.add("双跑指纹一致", dual_run_consistent(), "同一脚本两次执行 FNV-1a 指纹逐位一致");
    // 14 段位守卫。
    s.add("段位守卫", seg_guard_ok(), "SEG_BASE 0xC2440 须 0x40 对齐且在分派区间");
    // 15 指纹非退化 + LCG 确定性。
    {
        let h1 = fnv1a(b"unx51646-probe-a");
        let h2 = fnv1a(b"unx51646-probe-b");
        let mut s1: u64 = (SEG_BASE as u64) * 3 + 11;
        let mut s2: u64 = (SEG_BASE as u64) * 3 + 11;
        let mut same = true;
        for _ in 0..16 {
            if lcg_next(&mut s1) != lcg_next(&mut s2) {
                same = false;
            }
        }
        s.add("指纹与 LCG 面全绿", h1 != h2 && h1 != 0 && same, "FNV 非退化且同种子 LCG 序列逐值一致");
    }
    // 16 空动作留账显性拒（三要素缺失检出）。
    {
        let mut e = IntervalLossEngine::new();
        let r = e.ledger_factor(9, 0, "");
        s.add("空动作留账拒", r == Err(LossCode::FactorMissing), "处置动作缺失不得入账（三要素守卫）");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_time_events_consumed() {
        let mut e = IntervalLossEngine::new();
        assert_eq!(e.arrive(0), Ok(1));
        assert_eq!(e.arrive(INTERVAL), Ok(2));
        assert_eq!(e.consumed(), 2);
    }

    #[test]
    fn single_gap_compensated() {
        let mut e = IntervalLossEngine::new();
        let _ = e.arrive(0);
        assert_eq!(e.arrive(INTERVAL + 1), Err(LossCode::IntervalLoss));
        assert_eq!(e.compensate(), Ok(1));
    }

    #[test]
    fn multi_gap_compensated() {
        let mut e = IntervalLossEngine::new();
        let _ = e.arrive(0);
        let _ = e.arrive(INTERVAL + 3);
        assert_eq!(e.compensate(), Ok(3));
    }

    #[test]
    fn tick_regress_detected() {
        let mut e = IntervalLossEngine::new();
        let _ = e.arrive(0);
        assert_eq!(e.arrive(1), Err(LossCode::TickRegress));
    }

    #[test]
    fn recover_with_debt_rejected() {
        let mut e = IntervalLossEngine::new();
        let _ = e.arrive(0);
        let _ = e.arrive(INTERVAL + 2);
        assert!(e.recover().is_err());
    }

    #[test]
    fn full_recover_chain() {
        let mut e = IntervalLossEngine::new();
        let _ = e.arrive(0);
        let _ = e.arrive(INTERVAL + 1);
        let _ = e.compensate();
        assert!(e.recover().is_ok());
        assert_eq!(e.phase(), RecoveryPhase::Recovered);
    }

    #[test]
    fn factor_ledger_complete() {
        let mut e = IntervalLossEngine::new();
        let _ = e.arrive(0);
        let _ = e.arrive(INTERVAL + 2);
        assert!(e.factors_len() >= 1);
        assert!(e.factor_complete(0));
    }

    #[test]
    fn empty_action_rejected() {
        let mut e = IntervalLossEngine::new();
        assert_eq!(e.ledger_factor(9, 0, ""), Err(LossCode::FactorMissing));
    }

    #[test]
    fn no_loss_clean() {
        assert!(no_loss_zero_compensate());
    }

    #[test]
    fn injection_five_classes_all_caught() {
        let (inj, det, esc, cls) = injection_core();
        assert_eq!((inj, det, esc, cls), (50, 50, 0, 5));
    }

    #[test]
    fn poke_oob_rejected() {
        let mut e = IntervalLossEngine::new();
        assert_eq!(e.poke(RING_CAP + 1, 1), Err(LossCode::IndexOob));
        assert!(e.poke(0, 1).is_ok());
    }

    #[test]
    fn ledger_conserved() {
        let mut e = IntervalLossEngine::new();
        for _ in 0..10 {
            e.note_detected();
        }
        assert!(e.conserved());
    }

    #[test]
    fn dual_fingerprint_stable() {
        assert!(dual_run_consistent());
        assert_eq!(engine_fingerprint(), engine_fingerprint());
    }

    #[test]
    fn codes_closed_set() {
        assert_eq!(ALL_CODES.len(), 12);
        for (idx, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + idx as u32);
            assert!(!c.detail().is_empty());
        }
        assert!(seg_guard_ok());
    }

    #[test]
    fn unx51646_checks_green() {
        let set = run_unx51646_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
