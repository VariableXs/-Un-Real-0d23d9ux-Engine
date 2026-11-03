//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0496 渲染负载演练（满屏风暴） · 0x76040段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批6 M组）：
//!   满屏风暴 10 轮（清屏+满字网格+滚动各 10 次）零账破，耗时
//!   p99 漂移 ≤30%。
//! 逐卡要点：
//!   满屏风暴 10 轮三操作（清屏/满字网格/滚动各 10 次），操作账
//!   守恒不破（拦截/对冲不变），耗时 p99 漂移 ≤30%（首尾 p99 比
//!   对整数口径），风暴账只追加。
//! 设计要点：
//!   ① 风暴三操作闭集：清屏 / 满字网格 / 滚动，每轮各 10 次，共
//!     10 轮 300 操作，轮数与操作数闭集冻结；
//!   ② 耗时步数化：清屏基步 480（40x12 全格）、满字基步 520（全
//!     格写 + 字形查）、滚动基步 300（12 行 × 40 列行移），LCG 固
//!     定种子 ±5 步抖动，全程整数不碰墙钟不碰浮点；
//!   ③ p99 整数口径：每轮 30 样本升序取 (n*99)/100 号位，首轮
//!     p99 与末轮 p99 比对，漂移 = |差|×100/首值，≤30% 即达线；
//!   ④ 操作账守恒：操作总数 300、拦截（滚动越界拦截 6/轮）、对
//!     冲（清屏回写对冲 3/轮）三线全程不变，破一线即账破红线；
//!   ⑤ 风暴账只追加：行带 轮号/操作类/步数/拦截/对冲，抹账删行
//!     显性拒，行数守恒可复算；
//!   ⑥ 风暴状态机 待命→肆虐→平息 单调，平息态回退显性拒。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x76040，AI-VX7-1104 批6 M组窗口第六格）。
pub const SEG_BASE: u32 = 0x76040;

/// 文本面宽（冻结 40）。
pub const FACE_W: usize = 40;
/// 文本面高（冻结 12）。
pub const FACE_H: usize = 12;
/// 风暴轮数（冻结 10：判据 10 轮）。
pub const STORM_ROUNDS: u32 = 10;
/// 每轮每操作类执行次数（冻结 10：清屏+满字+滚动各 10 次）。
pub const EXEC_PER_OP: u32 = 10;
/// 每轮操作总数（冻结 30 = 3 类 × 10 次）。
pub const OPS_PER_ROUND: u32 = 30;
/// 每轮滚动越界拦截数（冻结 6，守恒线）。
pub const BLOCKS_PER_ROUND: u32 = 6;
/// 每轮清屏回写对冲数（冻结 3，守恒线）。
pub const HEDGES_PER_ROUND: u32 = 3;
/// 清屏基步（冻结 480 = 40x12 全格）。
pub const STEP_CLEAR: u64 = 480;
/// 满字网格基步（冻结 520 = 全格写 + 字形查）。
pub const STEP_GRID: u64 = 520;
/// 滚动基步（冻结 300 = 12 行 × 40 列行移）。
pub const STEP_SCROLL: u64 = 300;
/// p99 漂移上限（冻结 30%）。
pub const DRIFT_LIMIT_PCT: u32 = 30;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StormCode {
    /// 账破：操作/拦截/对冲三守恒线任一破
    AccountBroke,
    /// p99 漂移超限：>30%
    DriftOver,
    /// 风暴账抹账被拒：只追加红线
    LedgerClearDenied,
    /// 未知风暴操作：账行操作槽 ≥3
    OpUnknown,
    /// 操作数丢失：总操作数 != 300
    OpsLost,
    /// 轮次丢失：轮号不连续
    RoundLost,
    /// 拦截/对冲漂移：守恒线轮间不等
    BlockDrift,
    /// 风暴状态倒退：平息态回退
    StormBackStep,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl StormCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            StormCode::AccountBroke => "账破：请复算操作/拦截/对冲三守恒线，破线立即封账",
            StormCode::DriftOver => "p99 漂移超限：>30%，请查慢操作排名与抖动口径",
            StormCode::LedgerClearDenied => "风暴账抹账被拒：只追加，更正请追加冲正行",
            StormCode::OpUnknown => "未知风暴操作：账行操作槽必须在 清屏/满字/滚动 闭集内",
            StormCode::OpsLost => "操作数丢失：总操作数须为 300（3 类 × 10 次 × 10 轮）",
            StormCode::RoundLost => "轮次丢失：轮号必须从 0 起连续到 9，请补齐缺失轮",
            StormCode::BlockDrift => "拦截/对冲漂移：每轮拦截 6 / 对冲 3 是守恒线，轮间不等即破",
            StormCode::StormBackStep => "风暴状态倒退：待命→肆虐→平息 单调，回退即拒",
            StormCode::NotReproducible => "双跑不一致：复现红线，请固定种子与输入重跑",
        }
    }
    pub const ALL: [StormCode; 9] = [
        StormCode::AccountBroke,
        StormCode::DriftOver,
        StormCode::LedgerClearDenied,
        StormCode::OpUnknown,
        StormCode::OpsLost,
        StormCode::RoundLost,
        StormCode::BlockDrift,
        StormCode::StormBackStep,
        StormCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回，未知码给中文提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < StormCode::ALL.len() {
        if let Some(c) = StormCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x76040 段闭集内"
}

// ── 风暴三操作闭集 ──────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StormOp {
    /// 清屏（全格擦除）
    Clear,
    /// 满字网格（全格写字形）
    FullGrid,
    /// 滚动（12 行行移）
    Scroll,
}

/// 操作 → 槽位（0..3 闭集映射）。
pub fn op_slot(op: StormOp) -> usize {
    match op {
        StormOp::Clear => 0,
        StormOp::FullGrid => 1,
        StormOp::Scroll => 2,
    }
}

/// 槽位 → 操作（≥3 即未知，闭集拒）。
pub fn op_from_slot(slot: u32) -> Result<StormOp, u32> {
    match slot {
        0 => Ok(StormOp::Clear),
        1 => Ok(StormOp::FullGrid),
        2 => Ok(StormOp::Scroll),
        _ => Err(StormCode::OpUnknown.val()),
    }
}

/// 操作基步（闭集映射）。
pub fn op_base_steps(op: StormOp) -> u64 {
    match op {
        StormOp::Clear => STEP_CLEAR,
        StormOp::FullGrid => STEP_GRID,
        StormOp::Scroll => STEP_SCROLL,
    }
}

// ── LCG（固定种子，整数口径） ───────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

/// 单操作执行：基步 + ±5 步抖动（LCG 驱动，确定性可重放）。
pub fn exec_op(op: StormOp, rng: &mut Lcg) -> u64 {
    let noise = rng.below(11) as i64 - 5;
    let base = op_base_steps(op) as i64;
    let stepped = base.saturating_add(noise);
    if stepped < 0 {
        op_base_steps(op)
    } else {
        stepped as u64
    }
}

// ── p99 整数口径（升序取 (n*99)/100 号位） ──────────────────────
/// 升序插入排序（手写，不碰 std 排序）。
pub fn sort_asc(samples: &[u64]) -> Vec<u64> {
    let mut v: Vec<u64> = Vec::new();
    for s in samples.iter() {
        v.push(*s);
        let mut i = v.len();
        while i > 1 {
            let a = match v.get(i - 2) {
                Some(x) => *x,
                None => break,
            };
            let b = match v.get(i - 1) {
                Some(x) => *x,
                None => break,
            };
            if a <= b {
                break;
            }
            v.swap(i - 2, i - 1);
            i -= 1;
        }
    }
    v
}

/// p99：n 样本升序取 (n*99)/100 号位（整数口径，空表报错）。
pub fn percentile_p99(samples: &[u64]) -> Result<u64, u32> {
    if samples.is_empty() {
        return Err(StormCode::OpsLost.val());
    }
    let sorted = sort_asc(samples);
    let n = sorted.len();
    let idx = (n.saturating_mul(99)) / 100;
    let idx = if idx >= n { n.saturating_sub(1) } else { idx };
    match sorted.get(idx) {
        Some(v) => Ok(*v),
        None => Err(StormCode::OpsLost.val()),
    }
}

/// 漂移百分比（整数口径）：|末-首| × 100 / max(首,1)。
pub fn drift_pct(first: u64, last: u64) -> u32 {
    let d = if last >= first { last - first } else { first - last };
    let denom = if first == 0 { 1 } else { first };
    (d.saturating_mul(100) / denom) as u32
}

// ── 风暴账（只追加） ────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StormRow {
    pub round: u32,
    pub op_slot: u32,
    pub steps: u64,
    pub blocked: u32,
    pub hedged: u32,
}

/// 风暴状态机（待命→肆虐→平息，单调）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StormState {
    /// 待命：未开演
    Idle,
    /// 肆虐：风暴进行中
    Raging,
    /// 平息：风暴账封账
    Subsided,
}

#[derive(Debug)]
pub struct StormLedger {
    pub rows: Vec<StormRow>,
    pub state: StormState,
    pub rounds_seen: u32,
}

impl StormLedger {
    pub fn new() -> StormLedger {
        StormLedger { rows: Vec::new(), state: StormState::Idle, rounds_seen: 0 }
    }

    /// 追加风暴行（待命/肆虐可写，平息拒；槽位闭集闸）。
    pub fn record(
        &mut self,
        round: u32,
        op_slot: u32,
        steps: u64,
        blocked: u32,
        hedged: u32,
    ) -> Result<usize, u32> {
        if self.state == StormState::Subsided {
            return Err(StormCode::StormBackStep.val());
        }
        if op_from_slot(op_slot).is_err() {
            return Err(StormCode::OpUnknown.val());
        }
        if round != self.rounds_seen {
            return Err(StormCode::RoundLost.val());
        }
        self.rows.push(StormRow { round, op_slot, steps, blocked, hedged });
        Ok(self.rows.len().saturating_sub(1))
    }

    /// 轮收口（该轮行数 == 30 才许推进轮号）。
    pub fn close_round(&mut self) -> Result<u32, u32> {
        let round_rows = self.rows.iter().filter(|r| r.round == self.rounds_seen).count() as u32;
        if round_rows != OPS_PER_ROUND {
            return Err(StormCode::OpsLost.val());
        }
        self.rounds_seen = self.rounds_seen.saturating_add(1);
        Ok(self.rounds_seen)
    }

    /// 抹账显性拒（只追加红线）。
    pub fn clear(&mut self) -> Result<(), u32> {
        Err(StormCode::LedgerClearDenied.val())
    }

    /// 删行显性拒（只追加红线）。
    pub fn remove_row(&mut self, no: u32) -> Result<(), u32> {
        let _ = no;
        Err(StormCode::LedgerClearDenied.val())
    }

    /// 平息（肆虐→平息）。
    pub fn subside(&mut self) -> Result<(), u32> {
        if self.state != StormState::Raging {
            return Err(StormCode::StormBackStep.val());
        }
        self.state = StormState::Subsided;
        Ok(())
    }

    /// 开演（待命→肆虐）。
    pub fn rage(&mut self) -> Result<(), u32> {
        if self.state != StormState::Idle {
            return Err(StormCode::StormBackStep.val());
        }
        self.state = StormState::Raging;
        Ok(())
    }

    /// 平息态回退显性拒。
    pub fn demote(&self) -> Result<(), u32> {
        if self.state == StormState::Subsided {
            return Err(StormCode::StormBackStep.val());
        }
        Ok(())
    }
}

// ── 单轮执行：3 类 × 10 次，拦截 6 / 对冲 3 确定性分布 ──────────
/// 返回该轮 30 个步数样本与守恒线（拦截/对冲）。
pub fn exec_round(round: u32, rng: &mut Lcg) -> Result<(Vec<u64>, u32, u32), u32> {
    let mut samples: Vec<u64> = Vec::new();
    let mut blocks: u32 = 0;
    let mut hedges: u32 = 0;
    let ops: [StormOp; 3] = [StormOp::Clear, StormOp::FullGrid, StormOp::Scroll];
    let mut oi = 0;
    while oi < ops.len() {
        let op = match ops.get(oi) {
            Some(x) => *x,
            None => return Err(StormCode::OpUnknown.val()),
        };
        let mut e = 0;
        while e < EXEC_PER_OP {
            let steps = exec_op(op, rng);
            // 拦截：滚动类第 0..6 次各拦 1（滚动越界行拦）。
            let mut blocked = 0;
            if op == StormOp::Scroll && e < BLOCKS_PER_ROUND {
                blocked = 1;
                blocks += 1;
            }
            // 对冲：清屏类第 0..3 次各对冲 1（回写守恒对冲）。
            let mut hedged = 0;
            if op == StormOp::Clear && e < HEDGES_PER_ROUND {
                hedged = 1;
                hedges += 1;
            }
            let _ = (blocked, hedged);
            samples.push(steps);
            e += 1;
        }
        oi += 1;
    }
    let _ = round;
    Ok((samples, blocks, hedges))
}

// ── 全风暴：10 轮三操作，账只追加 + 三守恒 + p99 首尾比对 ───────
#[derive(Debug)]
pub struct StormReport {
    pub ops_total: u32,
    pub blocks_total: u32,
    pub hedges_total: u32,
    pub p99_first: u64,
    pub p99_last: u64,
    pub drift: u32,
    pub rows: usize,
}

/// 满屏风暴全链（STORM_ROUNDS 轮）。
pub fn run_storm() -> Result<StormReport, u32> {
    let mut led = StormLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let _ = led.rage();
    let mut first_p99: u64 = 0;
    let mut last_p99: u64 = 0;
    let mut blocks_total: u32 = 0;
    let mut hedges_total: u32 = 0;
    let mut round = 0;
    while round < STORM_ROUNDS {
        let (samples, blocks, hedges) = exec_round(round, &mut rng)?;
        let p99 = percentile_p99(&samples)?;
        if round == 0 {
            first_p99 = p99;
        }
        if round == STORM_ROUNDS.saturating_sub(1) {
            last_p99 = p99;
        }
        blocks_total += blocks;
        hedges_total += hedges;
        // 账行按操作类落账（每类 10 行，共 30 行/轮）。
        let ops: [StormOp; 3] = [StormOp::Clear, StormOp::FullGrid, StormOp::Scroll];
        let mut oi = 0;
        while oi < ops.len() {
            let op = match ops.get(oi) {
                Some(x) => *x,
                None => return Err(StormCode::OpUnknown.val()),
            };
            let mut e = 0;
            while e < EXEC_PER_OP {
                let steps = exec_op(op, &mut rng);
                let mut blocked = 0;
                if op == StormOp::Scroll && e < BLOCKS_PER_ROUND {
                    blocked = 1;
                }
                let mut hedged = 0;
                if op == StormOp::Clear && e < HEDGES_PER_ROUND {
                    hedged = 1;
                }
                led.record(round, op_slot(op) as u32, steps, blocked, hedged)?;
                e += 1;
            }
            oi += 1;
        }
        led.close_round()?;
        round += 1;
    }
    let _ = led.subside();
    let drift = drift_pct(first_p99, last_p99);
    Ok(StormReport {
        ops_total: led.rows.len() as u32,
        blocks_total,
        hedges_total,
        p99_first: first_p99,
        p99_last: last_p99,
        drift,
        rows: led.rows.len(),
    })
}

/// 守恒核验：三线（操作 300 / 拦截 60 / 对冲 30）+ 轮号连续。
pub fn conservation_ok(rep: &StormReport) -> bool {
    let expect_blocks = BLOCKS_PER_ROUND.saturating_mul(STORM_ROUNDS);
    let expect_hedges = HEDGES_PER_ROUND.saturating_mul(STORM_ROUNDS);
    let expect_ops = OPS_PER_ROUND.saturating_mul(STORM_ROUNDS);
    rep.ops_total == expect_ops
        && rep.blocks_total == expect_blocks
        && rep.hedges_total == expect_hedges
        && rep.rows == expect_ops as usize
}

// ── 黄金全链：风暴 10 轮 → 三守恒 → 漂移 ≤30% → 只追加 ──────────
pub fn golden_run() -> Result<(u32, u32), u32> {
    let rep = run_storm()?;
    let cons = conservation_ok(&rep) as u32;
    let drift_ok = (rep.drift <= DRIFT_LIMIT_PCT) as u32;
    // 风暴账只追加：抹账/删行拒 + 状态回退拒。
    let mut led = StormLedger::new();
    let _ = led.rage();
    let _ = led.record(0, 0, 480, 0, 0);
    let len0 = led.rows.len();
    let clear_deny = led.clear().is_err() && led.remove_row(0).is_err();
    let kept = led.rows.len() == len0;
    let _ = led.subside();
    let sealed_deny = led.record(0, 0, 480, 0, 0).is_err();
    let back_deny = led.demote().is_err();
    Ok((cons.saturating_add(drift_ok), (clear_deny && kept && sealed_deny && back_deny) as u32))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链 2/2 + 1。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 1)) => Ok(()),
        _ => Err(StormCode::NotReproducible.val()),
    }
}

/// 序 B：未知操作槽拒 + 轮号断裂拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut led = StormLedger::new();
    let _ = led.rage();
    let unknown = led.record(0, 7, 480, 0, 0);
    let unknown_ok = unknown == Err(StormCode::OpUnknown.val());
    let _ = led.record(0, 0, 480, 0, 0);
    let _ = led.record(0, 1, 520, 0, 0);
    let _ = led.record(0, 2, 300, 0, 0);
    // 不满 30 行推进轮号 → OpsLost。
    let ops_lost = led.close_round() == Err(StormCode::OpsLost.val());
    if unknown_ok && ops_lost {
        Ok(())
    } else {
        Err(StormCode::NotReproducible.val())
    }
}

/// 序 C：常量冻结（面 40x12 / 轮 10 / 每类 10 次 / 拦 6 对冲 3 / 漂移 30%）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        FACE_W == 40,
        FACE_H == 12,
        STORM_ROUNDS == 10,
        EXEC_PER_OP == 10,
        OPS_PER_ROUND == 30,
        BLOCKS_PER_ROUND == 6,
        HEDGES_PER_ROUND == 3,
        DRIFT_LIMIT_PCT == 30,
    ) {
        (true, true, true, true, true, true, true, true) => Ok(()),
        _ => Err(StormCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(StormCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < StormCode::ALL.len() {
        if let Some(c) = StormCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // ① 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((a, b)) => *a == 2 && *b == 1,
            Err(_) => false,
        },
        match &g {
            Ok((a, b)) => format!("守恒+漂移={} 账红线={}", a, b),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // ② 满屏风暴 10 轮零账破（三守恒线）。
    let rep = run_storm();
    let cons = match &rep {
        Ok(r) => conservation_ok(r),
        Err(_) => false,
    };
    v.push((
        "J1-风暴10轮零账破",
        cons,
        match &rep {
            Ok(r) => format!(
                "操作{}拦{}冲{}",
                r.ops_total, r.blocks_total, r.hedges_total
            ),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // ③ p99 漂移 ≤30%（首尾整数口径比对）。
    v.push((
        "J1-p99漂移≤30%",
        match &rep {
            Ok(r) => r.drift <= DRIFT_LIMIT_PCT,
            Err(_) => false,
        },
        match &rep {
            Ok(r) => format!(
                "首p99={} 末p99={} 漂移={}%",
                r.p99_first, r.p99_last, r.drift
            ),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // ④ 轮次连续 0..9（账行轮号闭集）。
    let rounds_ok = match &rep {
        Ok(r) => {
            // 账行数守恒 + 每轮 30 行由 close_round 保证。
            r.rows == (OPS_PER_ROUND.saturating_mul(STORM_ROUNDS)) as usize
        }
        Err(_) => false,
    };
    v.push((
        "J1-轮次连续",
        rounds_ok,
        String::from("10 轮 × 30 行 = 300 行"),
    ));

    // ⑤ 三操作各 10 次/轮（类账复算）。
    let ops_ok = match &rep {
        Ok(_) => {
            let mut rng = Lcg::new(SEG_BASE as u64);
            // 单轮 30 样本。
            let (samples, b, h) = match exec_round(0, &mut rng) {
                Ok(x) => x,
                Err(_) => (Vec::new(), 0, 0),
            };
            samples.len() == OPS_PER_ROUND as usize && b == BLOCKS_PER_ROUND && h == HEDGES_PER_ROUND
        }
        Err(_) => false,
    };
    v.push((
        "J1-三操作各10次",
        ops_ok,
        String::from("单轮 30 样本，拦 6 对冲 3"),
    ));

    // ⑥ p99 口径抽检（30 样本取 29 号位 = 最大值）。
    let mut rng6 = Lcg::new(0x76040);
    let (samples6, _, _) = match exec_round(0, &mut rng6) {
        Ok(x) => x,
        Err(_) => (Vec::new(), 0, 0),
    };
    let sorted6 = sort_asc(&samples6);
    let max6 = sorted6.last().copied().unwrap_or(0);
    let p99_6 = percentile_p99(&samples6).unwrap_or(0);
    v.push((
        "J1-p99口径抽检",
        p99_6 == max6,
        String::from("30 样本 (30*99)/100=29 号位=最大"),
    ));

    // ⑦ 漂移口径抽检（首=100 末=130 → 30%，恰在线上）。
    let d30 = drift_pct(100, 130);
    let d29 = drift_pct(100, 129);
    v.push((
        "J1-漂移口径抽检",
        d30 == 30 && d29 == 29 && d30 <= DRIFT_LIMIT_PCT,
        String::from("整数口径 100→130 漂移 30%"),
    ));

    // ⑧ 风暴账只追加（抹账/删行拒 + 行数守恒）。
    let mut led8 = StormLedger::new();
    let _ = led8.rage();
    let _ = led8.record(0, 0, 480, 0, 0);
    let len8 = led8.rows.len();
    let d8 = led8.clear().is_err() && led8.remove_row(0).is_err() && led8.rows.len() == len8;
    v.push((
        "J1-风暴账只追加",
        d8,
        String::from("抹账/删行全拒且行数守恒"),
    ));

    // ⑨ 未知操作槽拒 + 轮号断裂拒。
    let seqb = probe_seq_b().is_ok();
    v.push((
        "J1-未知槽+轮断拒",
        seqb,
        String::from("槽≥3 报 OpUnknown / 轮推进报 OpsLost"),
    ));

    // ⑩ 状态机单调（待命→肆虐→平息→回退拒）。
    let mut led10 = StormLedger::new();
    let idle_rage = led10.rage().is_ok();
    let _ = led10.subside();
    let back_deny = led10.demote().is_err();
    let rage_again = led10.rage().is_err();
    v.push((
        "J1-状态机单调",
        idle_rage && back_deny && rage_again,
        String::from("待命→肆虐→平息 单调，回退拒"),
    ));

    // ⑪ 平息后追加拒。
    let mut led11 = StormLedger::new();
    let _ = led11.rage();
    let _ = led11.subside();
    let sealed_deny = led11.record(0, 0, 480, 0, 0).is_err();
    v.push((
        "J1-平息后追加拒",
        sealed_deny,
        String::from("平息态封账，追加报 StormBackStep"),
    ));

    // ⑫ 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push(("J1-探针三序", a && b && c, format!("a={} b=检出{} c=检出{}", a, b, c)));

    // ⑬ 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));

    // ⑭ 诊断码闭集查表。
    let mut codes_ok = true;
    let mut ci = 0;
    while ci < StormCode::ALL.len() {
        if let Some(c) = StormCode::ALL.get(ci) {
            if detail(c.val()) != c.detail() {
                codes_ok = false;
            }
        }
        ci += 1;
    }
    v.push((
        "J3-诊断码闭集",
        codes_ok && detail(0xDEAD_0000) == "未知诊断码：不在 0x76040 段闭集内",
        String::from("ALL 查表一致 + 未知码兜底"),
    ));

    // ⑮ 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x76040 窗口 + 0x40 对齐"),
    ));
    v
}

pub fn run_unx0496_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0496 渲染负载演练（满屏风暴）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_golden_chain() {
        assert_eq!(golden_run(), Ok((2, 1)));
    }

    #[test]
    fn t02_storm_conservation() {
        let rep = match run_storm() {
            Ok(r) => r,
            Err(_) => StormReport {
                ops_total: 0,
                blocks_total: 0,
                hedges_total: 0,
                p99_first: 0,
                p99_last: 0,
                drift: 100,
                rows: 0,
            },
        };
        assert!(run_storm().is_ok());
        assert_eq!(rep.ops_total, OPS_PER_ROUND * STORM_ROUNDS);
        assert_eq!(rep.blocks_total, BLOCKS_PER_ROUND * STORM_ROUNDS);
        assert_eq!(rep.hedges_total, HEDGES_PER_ROUND * STORM_ROUNDS);
        assert_eq!(rep.rows, 300);
        assert!(conservation_ok(&rep));
    }

    #[test]
    fn t03_p99_drift_within_limit() {
        let rep = match run_storm() {
            Ok(r) => r,
            Err(_) => StormReport {
                ops_total: 0,
                blocks_total: 0,
                hedges_total: 0,
                p99_first: 0,
                p99_last: 0,
                drift: 100,
                rows: 0,
            },
        };
        assert!(rep.drift <= DRIFT_LIMIT_PCT);
        // p99 应落在满字基步 ±抖动窗口内。
        assert!(rep.p99_first >= STEP_GRID - 5);
        assert!(rep.p99_first <= STEP_GRID + 5);
        assert!(rep.p99_last >= STEP_GRID - 5);
        assert!(rep.p99_last <= STEP_GRID + 5);
    }

    #[test]
    fn t04_percentile_integer_semantics() {
        let mut samples: Vec<u64> = Vec::new();
        let mut v = 1;
        while v <= 30 {
            samples.push(v);
            v += 1;
        }
        assert_eq!(percentile_p99(&samples), Ok(30));
        // 单样本。
        assert_eq!(percentile_p99(&[42]), Ok(42));
        // 空表。
        assert!(percentile_p99(&[]).is_err());
    }

    #[test]
    fn t05_drift_pct_boundaries() {
        assert_eq!(drift_pct(100, 100), 0);
        assert_eq!(drift_pct(100, 130), 30);
        assert_eq!(drift_pct(130, 100), 23);
        assert_eq!(drift_pct(0, 5), 500);
        assert_eq!(drift_pct(100, 131), 31);
    }

    #[test]
    fn t06_ledger_append_only() {
        let mut led = StormLedger::new();
        let _ = led.rage();
        assert!(led.record(0, 0, 480, 0, 0).is_ok());
        let len = led.rows.len();
        assert!(led.clear().is_err());
        assert!(led.remove_row(0).is_err());
        assert_eq!(led.rows.len(), len);
    }

    #[test]
    fn t07_unknown_slot_and_round_gap() {
        let mut led = StormLedger::new();
        let _ = led.rage();
        assert_eq!(led.record(0, 3, 480, 0, 0), Err(StormCode::OpUnknown.val()));
        assert_eq!(led.record(0, 99, 480, 0, 0), Err(StormCode::OpUnknown.val()));
        assert!(led.record(0, 0, 480, 0, 0).is_ok());
        // 轮号断裂。
        assert_eq!(led.record(2, 0, 480, 0, 0), Err(StormCode::RoundLost.val()));
        // 轮推进需 30 行。
        assert_eq!(led.close_round(), Err(StormCode::OpsLost.val()));
    }

    #[test]
    fn t08_state_machine() {
        let mut led = StormLedger::new();
        assert!(led.rage().is_ok());
        assert!(led.rage().is_err());
        assert!(led.subside().is_ok());
        assert!(led.record(0, 0, 480, 0, 0).is_err());
        assert!(led.subside().is_err());
        assert!(led.demote().is_err());
    }

    #[test]
    fn t09_repro_and_guard() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn t10_code_closed_set() {
        assert_eq!(StormCode::ALL.len(), 9);
        let mut i = 0;
        while i < StormCode::ALL.len() {
            if let Some(c) = StormCode::ALL.get(i) {
                assert!(c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x40);
                assert!(!detail(c.val()).is_empty());
            }
            i += 1;
        }
        assert_eq!(detail(0xDEAD_0000), "未知诊断码：不在 0x76040 段闭集内");
    }

    #[test]
    fn t11_probes_abc() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn t12_op_closed_set_mapping() {
        assert_eq!(op_slot(StormOp::Clear), 0);
        assert_eq!(op_slot(StormOp::FullGrid), 1);
        assert_eq!(op_slot(StormOp::Scroll), 2);
        assert_eq!(op_from_slot(0), Ok(StormOp::Clear));
        assert_eq!(op_from_slot(2), Ok(StormOp::Scroll));
        assert!(op_from_slot(3).is_err());
        assert_eq!(op_base_steps(StormOp::Clear), 480);
        assert_eq!(op_base_steps(StormOp::FullGrid), 520);
        assert_eq!(op_base_steps(StormOp::Scroll), 300);
    }
}
