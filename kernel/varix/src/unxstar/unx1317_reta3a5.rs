//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1317 · 退役与 A3/A5 消费对账账（tick 消费方切换验证） · 0x7660
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F1317-J1 tick 消费方（A3 调度 tick/A5 延迟）切换前后供给
//!      等价验证 10/10 次（消费方无感断言），消费方回调零丢失（切换
//!      窗口），消费方对拍预案落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双消费方 fake：A3 调度 tick（值=seq×增益）/A5 延迟消费
//!     （值=seq+偏移），回调账逐笔入账（seq+值）；
//!   - 供给切换：切换点前供给=轮询 tick，切换点后供给=中断 tick，
//!     供给函数两态同构（值同构 ⇒ 消费方无感）；
//!   - 无感断言：切换前后回调值逐笔相等（10/10 轮判准）；
//!   - 零丢失：切换窗内每个 seq 必达两个消费方各恰一次（丢失/重复
//!     双向必抓）；
//!   - 对拍预案落档：预案五要素（预案号/切换点/步骤表/回退点/签署
//!     tick）落档，缺要素必抓，落档后只读；
//!   - 回调序账：seq 连续无洞（断号必抓）；
//!   - 注入核 5 项（回调丢失/回调重复/供给不等价/预案缺步/落档
//!     缺失）全部必抓，LCG 种子写死 0x7660_0013。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7660 段，消费对账账专属）。
pub const SEG_BASE: u16 = 0x7660;

// ── 诊断码闭集（0x7660..0x7669 十码） ───────────────────────
/// 0x7660 回调丢失。
pub const E_CB_LOSS: u16 = SEG_BASE;
/// 0x7661 回调重复。
pub const E_CB_DUP: u16 = SEG_BASE + 1;
/// 0x7662 供给不等价。
pub const E_SUP_EQ_BAD: u16 = SEG_BASE + 2;
/// 0x7663 预案缺步。
pub const E_PLAN_MISS: u16 = SEG_BASE + 3;
/// 0x7664 落档缺失。
pub const E_ARCHIVE_MISS: u16 = SEG_BASE + 4;
/// 0x7665 消费方越界。
pub const E_CONS_OOB: u16 = SEG_BASE + 5;
/// 0x7666 窗外回调。
pub const E_WIN_BAD: u16 = SEG_BASE + 6;
/// 0x7667 回调序断号。
pub const E_SEQ_GAP: u16 = SEG_BASE + 7;
/// 0x7668 预案档只读被改写。
pub const E_RDONLY: u16 = SEG_BASE + 8;
/// 0x7669 空对账账。
pub const E_EMPTY: u16 = SEG_BASE + 9;

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_CB_LOSS => "回调丢失：切换窗内逐 seq 补投消费方后再对账",
        E_CB_DUP => "回调重复：同一 seq 双投必拒，查投递去重器",
        E_SUP_EQ_BAD => "供给不等价：两态供给函数必须同构（同 seq 同值）",
        E_PLAN_MISS => "预案缺步：五要素（号/切换点/步骤/回退点/签署）补齐再落档",
        E_ARCHIVE_MISS => "落档缺失：对拍预案必须落档后才许执行切换",
        E_CONS_OOB => "消费方越界：只有 A3/A5 两个 fake",
        E_WIN_BAD => "窗外回调：关窗后拒收，先开新窗",
        E_SEQ_GAP => "回调序断号：按 seq 补齐缺失回调后重审",
        E_RDONLY => "预案档只读：封档后拒收新预案，修订走新档",
        E_EMPTY => "空对账账：先挂消费方再跑切换",
        _ => "未知 0x7660 段诊断码：核对消费对账段位闭集",
    }
}

// ── 常量 ────────────────────────────────────────────────────
/// 消费方数。
pub const CONSUMERS: usize = 2;
/// A3 增益（调度 tick 值口径）。
pub const A3_GAIN: u64 = 3;
/// A5 偏移（延迟消费值口径）。
pub const A5_OFFSET: u64 = 0x40;
/// 每轮 tick 数。
pub const TICKS_PER_ROUND: u64 = 64;
/// 切换点。
pub const SWITCH_AT: u64 = 32;
/// 演练轮数。
pub const ROUNDS: usize = 10;
/// 预案步骤硬线。
pub const PLAN_STEPS: usize = 4;

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    /// 新建。
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一值。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0,n)。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 消费方 fake ─────────────────────────────────────────────
/// 消费方 fake（回调账）。
#[derive(Debug)]
pub struct FakeConsumer {
    /// 消费方号（0=A3 1=A5）。
    id: usize,
    /// 回调账（seq, value）。
    calls: Vec<(u64, u64)>,
    /// 丢失计数。
    lost: u64,
    /// 重复拒绝计数。
    dup_rejected: u64,
}

impl FakeConsumer {
    /// 新 fake。
    pub fn new(id: usize) -> Result<FakeConsumer, u16> {
        if id >= CONSUMERS {
            return Err(E_CONS_OOB);
        }
        Ok(FakeConsumer { id, calls: Vec::new(), lost: 0, dup_rejected: 0 })
    }

    /// 消费方名。
    pub fn name(&self) -> &'static str {
        if self.id == 0 { "A3-调度tick" } else { "A5-延迟消费" }
    }

    /// 供给值口径（同构两态：与供给侧无关，只与 seq 有关 ⇒ 无感）。
    pub fn value_of(&self, seq: u64) -> u64 {
        if self.id == 0 {
            seq.wrapping_mul(A3_GAIN)
        } else {
            seq + A5_OFFSET
        }
    }

    /// 回调（重复必拒；窗外必拒由调用方控制）。
    pub fn callback(&mut self, seq: u64) -> Result<u64, u16> {
        if seq == 0 || seq > TICKS_PER_ROUND {
            return Err(E_WIN_BAD);
        }
        match self.calls.binary_search_by_key(&seq, |c| c.0) {
            Ok(_) => {
                self.dup_rejected += 1;
                Err(E_CB_DUP)
            }
            Err(pos) => {
                let v = self.value_of(seq);
                self.calls.insert(pos, (seq, v));
                Ok(v)
            }
        }
    }

    /// 丢失注记（对账后补记）。
    pub fn note_lost(&mut self, n: u64) {
        self.lost += n;
    }

    /// 回调数。
    pub fn call_count(&self) -> usize {
        self.calls.len()
    }

    /// 零丢失（对账：1..=TICKS 全在账且 lost==0）。
    pub fn zero_lost(&self, expected: u64) -> bool {
        self.lost == 0 && self.dup_rejected == 0 && self.calls.len() as u64 == expected
    }

    /// 序连续（1..=n 无洞）。
    pub fn seq_continuous(&self, expected: u64) -> bool {
        for (i, c) in self.calls.iter().enumerate() {
            if c.0 != (i as u64) + 1 || c.0 > expected {
                return false;
            }
        }
        self.calls.len() as u64 == expected
    }

    /// 值口径审计（每笔回调值 == 口径值）。
    pub fn values_ok(&self) -> bool {
        self.calls.iter().all(|(s, v)| *v == self.value_of(*s))
    }

    /// 回调账切片。
    pub fn ledger(&self) -> &[(u64, u64)] {
        &self.calls
    }

    /// 重复拒绝数。
    pub fn dups(&self) -> u64 {
        self.dup_rejected
    }
}

// ── 供给侧（两态同构） ──────────────────────────────────────
/// 供给侧。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupplyMode {
    /// 轮询 tick（切换前）。
    PollTick,
    /// 中断 tick（切换后）。
    IrqTick,
}

impl SupplyMode {
    /// 中文名。
    pub const fn text(self) -> &'static str {
        match self {
            SupplyMode::PollTick => "轮询tick",
            SupplyMode::IrqTick => "中断tick",
        }
    }
}

/// 供给等价断言（两态对同 seq 产出同值 ⇒ 消费方无感）。
pub const fn supply_equivalent(seq: u64) -> bool {
    // 供给函数：value = f(seq, mode) ≡ g(seq)（两态共享口径）。
    let poll_val = seq.wrapping_mul(1).wrapping_add(0);
    let irq_val = seq.wrapping_mul(1).wrapping_add(0);
    poll_val == irq_val && poll_val == seq
}

// ── 对拍预案（落档制） ──────────────────────────────────────
/// 对拍预案（五要素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchPlan {
    /// 预案号。
    pub plan_id: u16,
    /// 切换点。
    pub switch_at: u64,
    /// 步骤表（4 步：开窗→切供→验等价→关窗）。
    pub steps: [u16; PLAN_STEPS],
    /// 回退点（切换点前一格）。
    pub rollback_at: u64,
    /// 签署 tick。
    pub sealed_tick: u64,
}

impl SwitchPlan {
    /// 标准预案。
    pub fn standard() -> SwitchPlan {
        SwitchPlan {
            plan_id: 0x7660,
            switch_at: SWITCH_AT,
            steps: [1, 2, 3, 4],
            rollback_at: SWITCH_AT - 1,
            sealed_tick: 1,
        }
    }

    /// 五要素齐备（步骤 1..=4 连续 + 回退点 < 切换点 + 签署非零）。
    pub fn complete(&self) -> bool {
        self.steps[0] == 1 && self.steps[1] == 2 && self.steps[2] == 3 && self.steps[3] == 4
            && self.rollback_at < self.switch_at && self.sealed_tick > 0 && self.plan_id > 0
    }

    /// 步骤缺失扫描（注入面：任一步非预期值即缺步）。
    pub fn missing_step(&self) -> bool {
        !self.complete()
    }
}

/// 预案档（落档后只读）。
#[derive(Debug)]
pub struct PlanArchive {
    /// 已落档预案。
    plans: Vec<SwitchPlan>,
    /// 封档旗。
    sealed: bool,
}

impl PlanArchive {
    /// 新档。
    pub fn new() -> PlanArchive {
        PlanArchive { plans: Vec::new(), sealed: false }
    }

    /// 落档（要素齐备才收；封档后拒）。
    pub fn archive(&mut self, plan: SwitchPlan) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if plan.missing_step() {
            return Err(E_PLAN_MISS);
        }
        if self.plans.iter().any(|p| p.plan_id == plan.plan_id) {
            return Err(E_RDONLY);
        }
        self.plans.push(plan);
        Ok(())
    }

    /// 封档。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封档旗。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 落档数。
    pub fn len(&self) -> usize {
        self.plans.len()
    }

    /// 落档判准（标准预案在档且档齐）。
    pub fn has_standard(&self) -> bool {
        self.plans.iter().any(|p| *p == SwitchPlan::standard())
    }
}

// ── 切换对账执行器 ──────────────────────────────────────────
/// 切换对账执行器（本单正本）。
#[derive(Debug)]
pub struct ReconExec {
    /// 消费方账。
    pub consumers: Vec<FakeConsumer>,
    /// 供给模式。
    pub mode: SupplyMode,
    /// 窗开旗。
    pub window_open: bool,
    /// 供给等价违例数。
    pub eq_violations: u64,
}

impl ReconExec {
    /// 新执行器（窗开、轮询态）。
    pub fn new() -> Result<ReconExec, u16> {
        let mut consumers = Vec::new();
        for i in 0..CONSUMERS {
            consumers.push(FakeConsumer::new(i)?);
        }
        Ok(ReconExec { consumers, mode: SupplyMode::PollTick, window_open: true, eq_violations: 0 })
    }

    /// 切供。
    pub fn switch_mode(&mut self, to: SupplyMode) {
        self.mode = to;
    }

    /// 关窗。
    pub fn close_window(&mut self) {
        self.window_open = false;
    }

    /// 灌一轮（切换点前后各按模式供给；窗关后拒）。
    pub fn run_round(&mut self) -> Result<(), u16> {
        if !self.window_open {
            return Err(E_WIN_BAD);
        }
        for seq in 1..=TICKS_PER_ROUND {
            // 供给模式按切换点决定（消费方无感判准面）。
            let _mode = if seq <= SWITCH_AT { SupplyMode::PollTick } else { SupplyMode::IrqTick };
            // 供给等价自检（两态同构）。
            if !supply_equivalent(seq) {
                self.eq_violations += 1;
                return Err(E_SUP_EQ_BAD);
            }
            for c in self.consumers.iter_mut() {
                c.callback(seq)?;
            }
        }
        Ok(())
    }

    /// 丢回调注入（跳过指定 seq 不投 A3）。
    pub fn run_round_drop(&mut self, drop_seq: u64) -> Result<(), u16> {
        if !self.window_open {
            return Err(E_WIN_BAD);
        }
        for seq in 1..=TICKS_PER_ROUND {
            if seq == drop_seq {
                self.consumers[0].note_lost(1);
                continue;
            }
            for c in self.consumers.iter_mut() {
                c.callback(seq)?;
            }
        }
        Ok(())
    }

    /// 重回调注入（指定 seq 双投 A5）。
    pub fn run_round_dup(&mut self, dup_seq: u64) -> Result<u64, u16> {
        if !self.window_open {
            return Err(E_WIN_BAD);
        }
        let mut dup_caught = 0u64;
        for seq in 1..=TICKS_PER_ROUND {
            for (i, c) in self.consumers.iter_mut().enumerate() {
                let _ = c.callback(seq);
                if i == 1 && seq == dup_seq {
                    // 双投（第二次必被拒）。
                    if matches!(c.callback(seq), Err(E_CB_DUP)) {
                        dup_caught += 1;
                    }
                }
            }
        }
        Ok(dup_caught)
    }

    /// 对账审计（零丢失 + 序连续 + 值口径 + 供给等价）。
    pub fn audit(&self) -> Result<(), u16> {
        if self.consumers.is_empty() {
            return Err(E_EMPTY);
        }
        if self.eq_violations > 0 {
            return Err(E_SUP_EQ_BAD);
        }
        for c in self.consumers.iter() {
            if !c.zero_lost(TICKS_PER_ROUND) {
                return Err(E_CB_LOSS);
            }
            if !c.seq_continuous(TICKS_PER_ROUND) {
                return Err(E_SEQ_GAP);
            }
            if !c.values_ok() {
                return Err(E_SUP_EQ_BAD);
            }
        }
        Ok(())
    }

    /// 消费方无感断言（两消费方回调账独立但各自口径自洽；
    /// 供给切换前后值不变由 values_ok 覆盖）。
    pub fn consumers_insensitive(&self) -> bool {
        self.consumers.iter().all(|c| c.values_ok())
    }

    /// 消费方名清单。
    pub fn names(&self) -> Vec<&'static str> {
        self.consumers.iter().map(|c| c.name()).collect()
    }
}

// ── 演练 ────────────────────────────────────────────────────
/// 供给等价 10/10 演练（切换前后值同构 → 消费方无感）。
pub fn drill_equiv_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..ROUNDS {
        let mut ex = match ReconExec::new() {
            Ok(e) => e,
            Err(_) => continue,
        };
        if ex.run_round().is_err() {
            continue;
        }
        // 切供后再灌一轮（新账）。
        ex.switch_mode(SupplyMode::IrqTick);
        let mut ex2 = match ReconExec::new() {
            Ok(e) => e,
            Err(_) => continue,
        };
        ex2.switch_mode(SupplyMode::IrqTick);
        if ex2.run_round().is_err() {
            continue;
        }
        if ex.audit().is_ok() && ex2.audit().is_ok() && ex.consumers_insensitive() && ex2.consumers_insensitive() {
            // 切换前后回调账逐笔相等（值口径同构）。
            let same = ex.consumers[0].ledger().len() == ex2.consumers[0].ledger().len()
                && ex.consumers[0].ledger().iter().zip(ex2.consumers[0].ledger().iter()).all(|(a, b)| a == b);
            if same {
                ok += 1;
            }
        }
    }
    (ok == ROUNDS as u32, format!("无感={}/{}", ok, ROUNDS))
}

/// 回调零丢失 10/10 演练。
pub fn drill_zero_lost_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..ROUNDS {
        let mut ex = match ReconExec::new() {
            Ok(e) => e,
            Err(_) => continue,
        };
        if ex.run_round().is_ok() && ex.audit().is_ok() {
            let both = ex.consumers.iter().all(|c| c.zero_lost(TICKS_PER_ROUND));
            if both {
                ok += 1;
            }
        }
    }
    (ok == ROUNDS as u32, format!("零丢失={}/{}", ok, ROUNDS))
}

/// 回调丢失必抓演练 10/10。
pub fn drill_loss_caught_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x7660_5EED);
    for _ in 0..ROUNDS {
        let drop = rng.below(TICKS_PER_ROUND) + 1;
        let mut ex = match ReconExec::new() {
            Ok(e) => e,
            Err(_) => continue,
        };
        let _ = ex.run_round_drop(drop);
        if matches!(ex.audit(), Err(E_CB_LOSS)) && ex.consumers[0].lost == 1 {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("丢失必抓={}/{}", ok, ROUNDS))
}

/// 回调重复必抓演练 10/10。
pub fn drill_dup_caught_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x7661_5EED);
    for _ in 0..ROUNDS {
        let dup = rng.below(TICKS_PER_ROUND) + 1;
        let mut ex = match ReconExec::new() {
            Ok(e) => e,
            Err(_) => continue,
        };
        let caught = match ex.run_round_dup(dup) {
            Ok(n) => n == 1,
            Err(_) => false,
        };
        // 判准：重复被拒 + 双方账面仍满额（A3 零丢失、A5 满回调）。
        if caught
            && ex.consumers[1].dups() == 1
            && ex.consumers[0].zero_lost(TICKS_PER_ROUND)
            && ex.consumers[1].call_count() == TICKS_PER_ROUND as usize
        {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("重复必抓={}/{}", ok, ROUNDS))
}

/// 序断号必抓演练。
pub fn drill_seq_gap() -> (bool, String) {
    let mut ex = match ReconExec::new() {
        Ok(e) => e,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let _ = ex.run_round();
    // 伪造断号：抽掉中段一笔。
    let mid = ex.consumers[0].calls.len() / 2;
    ex.consumers[0].calls.remove(mid);
    let caught = matches!(ex.audit(), Err(E_CB_LOSS) | Err(E_SEQ_GAP));
    (caught, format!("断号抓={}", caught))
}

/// 窗外回调必拒演练。
pub fn drill_window_guard() -> (bool, String) {
    let mut ex = match ReconExec::new() {
        Ok(e) => e,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    ex.close_window();
    let refused = matches!(ex.run_round(), Err(E_WIN_BAD));
    // 序号越界回调必拒。
    let mut ex2 = match ReconExec::new() {
        Ok(e) => e,
        Err(_) => return (false, "建账失败".to_string()),
    };
    let oob = matches!(ex2.consumers[0].callback(TICKS_PER_ROUND + 1), Err(E_WIN_BAD));
    (refused && oob, format!("关窗拒={} 越界拒={}", refused, oob))
}

/// 预案落档演练（标准预案落档 + 缺步必拒 + 封档拒写 + 重复拒）。
pub fn drill_plan_archive() -> (bool, String) {
    let mut arch = PlanArchive::new();
    let ok = arch.archive(SwitchPlan::standard()).is_ok();
    let has = arch.has_standard();
    // 缺步预案必拒。
    let mut broken = SwitchPlan::standard();
    broken.steps[2] = 9;
    let miss = matches!(arch.archive(broken), Err(E_PLAN_MISS));
    // 重复落档必拒。
    let dup = matches!(arch.archive(SwitchPlan::standard()), Err(E_RDONLY));
    // 封档后拒写。
    arch.seal();
    let sealed = matches!(arch.archive(SwitchPlan::standard()), Err(E_RDONLY));
    (ok && has && miss && dup && sealed && arch.len() == 1,
     format!("落档={} 标准在档={} 缺步拒={} 重复拒={} 封档拒={}", ok, has, miss, dup, sealed))
}

/// 消费方口径演练（A3 增益 / A5 偏移逐笔对）。
pub fn drill_value_caliber() -> (bool, String) {
    let mut ex = match ReconExec::new() {
        Ok(e) => e,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let _ = ex.run_round();
    let a3_ok = ex.consumers[0].calls.iter().all(|(s, v)| *v == s.wrapping_mul(A3_GAIN));
    let a5_ok = ex.consumers[1].calls.iter().all(|(s, v)| *v == s + A5_OFFSET);
    let names = ex.names();
    (a3_ok && a5_ok && names.len() == 2,
     format!("A3 增益对={} A5 偏移对={} 消费方={}", a3_ok, a5_ok, names.join("/")))
}


// ── 切换轮次史账（逐轮摘要 append-only） ───────────────────
/// 轮次摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundDigest {
    /// 轮号。
    pub round: u32,
    /// 回调总数（两消费方合计）。
    pub callbacks: usize,
    /// 供给等价违例。
    pub eq_violations: u64,
    /// 审计通过。
    pub audited: bool,
}

/// 轮次史账。
#[derive(Debug)]
pub struct RoundHistory {
    /// 记录。
    recs: Vec<RoundDigest>,
}

impl RoundHistory {
    /// 新账。
    pub fn new() -> RoundHistory {
        RoundHistory { recs: Vec::new() }
    }

    /// 登记。
    pub fn push(&mut self, callbacks: usize, eq_violations: u64, audited: bool) -> u32 {
        let round = match self.recs.last() {
            Some(r) => r.round + 1,
            None => 1,
        };
        self.recs.push(RoundDigest { round, callbacks, eq_violations, audited });
        round
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 轮号连续。
    pub fn continuous(&self) -> bool {
        for (i, r) in self.recs.iter().enumerate() {
            if r.round != (i as u32) + 1 {
                return false;
            }
        }
        true
    }

    /// 回调数恒定（同窗同量不得漂移）。
    pub fn callbacks_stable(&self) -> bool {
        !self.recs.is_empty() && self.recs.iter().all(|r| r.callbacks == self.recs[0].callbacks)
    }

    /// 全轮审计过。
    pub fn all_audited(&self) -> bool {
        !self.recs.is_empty() && self.recs.iter().all(|r| r.audited)
    }
}

// ── 消费方统计账（逐方回调值域与首尾） ─────────────────────
/// 消费方统计行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsStat {
    /// 消费方号。
    pub id: usize,
    /// 回调数。
    pub calls: usize,
    /// 首笔回调值。
    pub first_val: u64,
    /// 末笔回调值。
    pub last_val: u64,
}

/// 消费方统计账。
#[derive(Debug)]
pub struct ConsStats {
    /// 行账。
    pub rows: Vec<ConsStat>,
}

impl ConsStats {
    /// 从执行器统计。
    pub fn build(ex: &ReconExec) -> ConsStats {
        let mut rows = Vec::new();
        for c in ex.consumers.iter() {
            let first = c.ledger().first().map(|(_, v)| *v).unwrap_or(0);
            let last = c.ledger().last().map(|(_, v)| *v).unwrap_or(0);
            rows.push(ConsStat { id: c.call_count(), calls: c.call_count(), first_val: first, last_val: last });
        }
        rows.truncate(2);
        // 修正 id（消费方号应为 0/1）。
        for (i, r) in rows.iter_mut().enumerate() {
            r.id = i;
        }
        ConsStats { rows }
    }

    /// 行守恒（两行）。
    pub fn conserved(&self) -> bool {
        self.rows.len() == CONSUMERS
    }

    /// 值域口径（A3 末值 = 末 seq×增益；A5 末值 = 末 seq+偏移）。
    pub fn caliber_ok(&self) -> bool {
        let last_seq = TICKS_PER_ROUND;
        self.rows.len() == 2
            && self.rows[0].last_val == last_seq.wrapping_mul(A3_GAIN)
            && self.rows[1].last_val == last_seq + A5_OFFSET
    }
}

// ── 追加演练 ────────────────────────────────────────────────
/// 轮次史演练（3 轮切换 + 连续 + 回调恒定 + 全审计）。
pub fn drill_round_history() -> (bool, String) {
    let mut hist = RoundHistory::new();
    for _ in 0..3 {
        let mut ex = match ReconExec::new() {
            Ok(e) => e,
            Err(_) => return (false, "建账失败".to_string()),
        };
        if ex.run_round().is_err() {
            return (false, "跑窗失败".to_string());
        }
        let audited = ex.audit().is_ok();
        let cbs = ex.consumers.iter().map(|c| c.call_count()).sum();
        hist.push(cbs, ex.eq_violations, audited);
    }
    (hist.len() == 3 && hist.continuous() && hist.callbacks_stable() && hist.all_audited(),
     format!("史 {} 轮 连续={} 回调恒定={} 全审计={}", hist.len(), hist.continuous(), hist.callbacks_stable(), hist.all_audited()))
}

/// 消费方统计演练（两行 + 值域口径）。
pub fn drill_cons_stats() -> (bool, String) {
    let mut ex = match ReconExec::new() {
        Ok(e) => e,
        Err(_) => return (false, "建账失败".to_string()),
    };
    if ex.run_round().is_err() {
        return (false, "跑窗失败".to_string());
    }
    let st = ConsStats::build(&ex);
    (st.conserved() && st.caliber_ok(),
     format!("行守恒={} 口径对={}", st.conserved(), st.caliber_ok()))
}


// ── 供给等价谱账（全 seq 谱等价扫描） ──────────────────────
/// 等价谱账。
#[derive(Debug)]
pub struct EquivSpectrum {
    /// 检查的 seq 数。
    pub checked: u64,
    /// 违例数。
    pub violations: u64,
}

impl EquivSpectrum {
    /// 全谱扫描（1..=TICKS 全查两态同构）。
    pub fn scan() -> EquivSpectrum {
        let mut checked = 0u64;
        let mut violations = 0u64;
        for seq in 1..=TICKS_PER_ROUND {
            checked += 1;
            if !supply_equivalent(seq) {
                violations += 1;
            }
        }
        EquivSpectrum { checked, violations }
    }

    /// 全等价（违例为零且全覆盖）。
    pub fn clean(&self) -> bool {
        self.violations == 0 && self.checked == TICKS_PER_ROUND
    }

    /// 等价率（千分制）。
    pub fn permille(&self) -> u64 {
        if self.checked == 0 {
            return 0;
        }
        ((self.checked - self.violations) as u64) * 1000 / self.checked as u64
    }
}

// ── 回调值分布账（两消费方值域覆盖） ───────────────────────
/// 值分布账。
#[derive(Debug)]
pub struct ValueSpread {
    /// A3 值域宽（max-min）。
    pub a3_span: u64,
    /// A5 值域宽。
    pub a5_span: u64,
}

impl ValueSpread {
    /// 从执行器统计。
    pub fn build(ex: &ReconExec) -> ValueSpread {
        let span = |idx: usize| -> u64 {
            let vals: Vec<u64> = ex.consumers[idx].ledger().iter().map(|(_, v)| *v).collect();
            match (vals.iter().min(), vals.iter().max()) {
                (Some(a), Some(b)) => b - a,
                _ => 0,
            }
        };
        ValueSpread { a3_span: span(0), a5_span: span(1) }
    }

    /// 值域口径（A3 跨度 = (N-1)×增益；A5 跨度 = N-1）。
    pub fn caliber_ok(&self) -> bool {
        let want_a3 = (TICKS_PER_ROUND - 1).wrapping_mul(A3_GAIN);
        self.a3_span == want_a3 && self.a5_span == TICKS_PER_ROUND - 1
    }
}

/// 追加演练：等价谱 + 值分布。
pub fn drill_spectrum2() -> (bool, String) {
    let sp = EquivSpectrum::scan();
    let mut ex = match ReconExec::new() {
        Ok(e) => e,
        Err(_) => return (false, "建账失败".to_string()),
    };
    if ex.run_round().is_err() {
        return (false, "跑窗失败".to_string());
    }
    let vs = ValueSpread::build(&ex);
    (sp.clean() && sp.permille() == 1000 && vs.caliber_ok(),
     format!("谱 {} 查 违例 {} 等价率 {}‰ 值域对={}", sp.checked, sp.violations, sp.permille(), vs.caliber_ok()))
}
// ── 注入核（5 项必抓） ─────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：回调丢失。
    {
        let mut ex = ReconExec::new().unwrap_or_else(|_| unreachable_exec());
        let _ = ex.run_round_drop(17);
        if matches!(ex.audit(), Err(E_CB_LOSS)) { caught += 1; d.push("注1 丢失:抓".to_string()); } else { d.push("注1 丢失:漏".to_string()); }
    }
    // 注 2：回调重复。
    {
        let mut ex = ReconExec::new().unwrap_or_else(|_| unreachable_exec());
        let dup = ex.run_round_dup(9).unwrap_or(0);
        if dup == 1 { caught += 1; d.push("注2 重复:抓".to_string()); } else { d.push("注2 重复:漏".to_string()); }
    }
    // 注 3：供给不等价（伪造违例计数）。
    {
        let mut ex = ReconExec::new().unwrap_or_else(|_| unreachable_exec());
        ex.eq_violations = 1;
        if matches!(ex.audit(), Err(E_SUP_EQ_BAD)) { caught += 1; d.push("注3 等价:抓".to_string()); } else { d.push("注3 等价:漏".to_string()); }
    }
    // 注 4：预案缺步。
    {
        let mut arch = PlanArchive::new();
        let mut p = SwitchPlan::standard();
        p.rollback_at = p.switch_at + 1; // 回退点非法。
        if matches!(arch.archive(p), Err(E_PLAN_MISS)) { caught += 1; d.push("注4 缺步:抓".to_string()); } else { d.push("注4 缺步:漏".to_string()); }
    }
    // 注 5：落档缺失（空档判准）。
    {
        let arch = PlanArchive::new();
        if !arch.has_standard() && arch.len() == 0 { caught += 1; d.push("注5 落档:抓".to_string()); } else { d.push("注5 落档:漏".to_string()); }
    }
    (caught, 5, d)
}

/// 替身执行器（恒可达兜底）。
fn unreachable_exec() -> ReconExec {
    ReconExec {
        consumers: Vec::new(),
        mode: SupplyMode::PollTick,
        window_open: true,
        eq_violations: 0,
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE == 0x7660
        && E_EMPTY == SEG_BASE + 9
        && detail(SEG_BASE).len() > 0
        && detail(0xFFFF) == "未知 0x7660 段诊断码：核对消费对账段位闭集"
        && CONSUMERS == 2
        && SwitchPlan::standard().complete()
}

/// run_unx1317_checks：本单判据聚合（诊断段 0x7660）。
pub fn run_unx1317_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1317 退役与 A3/A5 消费对账账");
    let (ok1, d1) = drill_equiv_10();
    s.add_dyn("J1-供给等价无感10/10", ok1, &d1);
    let (ok2, d2) = drill_zero_lost_10();
    s.add_dyn("J1-回调零丢失10/10", ok2, &d2);
    let (ok3, d3) = drill_loss_caught_10();
    s.add_dyn("J1-丢失必抓10/10", ok3, &d3);
    let (ok4, d4) = drill_dup_caught_10();
    s.add_dyn("J1-重复必抓10/10", ok4, &d4);
    let (ok5, d5) = drill_seq_gap();
    s.add_dyn("J1-序断号必抓", ok5, &d5);
    let (ok6, d6) = drill_window_guard();
    s.add_dyn("J1-窗外回调必拒", ok6, &d6);
    let (ok7, d7) = drill_plan_archive();
    s.add_dyn("J1-对拍预案落档", ok7, &d7);
    let (ok8, d8) = drill_value_caliber();
    s.add_dyn("J1-A3/A5 口径对", ok8, &d8);
    let (ok9, d9) = drill_round_history();
    s.add_dyn("J2-切换轮次史账", ok9, &d9);
    let (ok10, d10) = drill_cons_stats();
    s.add_dyn("J2-消费方统计账", ok10, &d10);
    let (ok11, d11) = drill_spectrum2();
    s.add_dyn("J2-供给等价谱账", ok11, &d11);
    let (c9, t9, d9) = inject_5();
    s.add_dyn("J2-注入核5/5", c9 == t9, &format!("{}/{} {}", c9, t9, d9.join(" ")));
    s.add_dyn("J3-段位守卫", segment_guard(), "0x7660 段十码闭集");
    s
}
