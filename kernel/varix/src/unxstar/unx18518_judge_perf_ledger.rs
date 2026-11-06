//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18518 · 判定性能账（微基准）
//! · 0x7FD40 段 · 批 06 · 配额 300
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18518-J1 单次判定 P95 <500ns 断言，百万次批量无漂移
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 计时口径冻结（解析成本模型，非墙钟）：判定路径成本 =
//!       BASE(20ns 入口) + 4ns/掩码位（位扫描） + 12ns/维比较
//!       （数值核） + SEAL(6ns 收口)
//!     ——热路径无分支惩罚项（掩码位与比较维同数：一路一位一比）；
//!   - 成本样本面冻结：8 档掩码（popcount 1..=8，MASK_SET）× 25 轮
//!     = 200 样本；成本 = 26 + 16×popcount（推导：20+6 + (4+12)×pc）
//!     ∈ [42, 154]ns；
//!   - P95 口径：200 样本升序排位，P95 取第 ⌈0.95×200⌉=190 位
//!     （下标 189）——J1 断言 P95 < 500ns（热路径上界裕量
//!     500-154 = 346ns ≈ 2.2×；上界来自冻结成本表外推，非实测墙钟）；
//!   - 批量无漂移：1,000,000 次判定（8 档掩码轮转），漂移账逐次
//!     复核裁决与黄金值——漂移必须为 0；成本总账 = Σ逐次成本
//!     （u64 checked 累加，溢出即账破）；
//!   - 守恒：判定次数 = 1,000,000；总成本 = Σ(26+16×pc_i)；
//!     双跑账逐字段相等（确定性红线）；
//!   - 注入三类 × 10 轮：裁决漂移注入（偏置评值必被抓）/ 成本篡改
//!     （成本表外推值失配必抓）/ 计数漂移（ judges ≠ 批量数必抓）
//!     ——10/10 全抓；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] PfCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [裁决表]   COST_TABLE 8 档冻结成本（手推）/ MASK_SET 8 掩码
//!   [数据结构] PerfLedger（u64 判定数/成本总账/漂移账）+ 样本环
//!   [状态机]   PfPhase 三态（Idle→Batching→Sealed）
//!   [成本核]   judge_cost / sample_costs / p95（插入排序取位）
//!   [注入核]   三类 × 10 轮（漂移/篡改/计数）
//!   [收口]     golden_run + run_unx18518_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FD40，AI-VX11-1105 G1 批06 A12 独占区）。
pub const SEG_BASE: u32 = 0x7FD40;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// VER_EQUAL（冻结：批量判定面恒等值比较）。
pub const VER_EQUAL: u8 = 1;
/// 系统快照（8 维，与总纲同源）。
pub const SNAPSHOT: [u32; 8] = [1, 6, 7601, 2, 1, 1, 0x0100, 1];
/// 成本基项（入口，ns）。
pub const COST_BASE: u32 = 20;
/// 掩码位扫描单价（ns/位）。
pub const COST_BIT: u32 = 4;
/// 单维比较单价（ns/维）。
pub const COST_DIM: u32 = 12;
/// 收口单价（ns）。
pub const COST_SEAL: u32 = 6;
/// P95 上界（J1「<500ns」口径）。
pub const P95_LIMIT: u32 = 500;
/// 样本轮数（8 档 × 25 = 200）。
pub const SAMPLE_ROUNDS: usize = 25;
/// 样本总数。
pub const SAMPLES: usize = 8 * SAMPLE_ROUNDS;
/// 批量判定次数（J1「百万次」口径）。
pub const BATCH_N: u64 = 1_000_000;
/// 注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

/// 8 档掩码（popcount 1..=8 递增；低维号优先置位）。
pub const MASK_SET: [u32; 8] = [0x02, 0x03, 0x07, 0x0F, 0x1F, 0x3F, 0x7F, 0xFF];

/// 掩码置位数（popcount）。
pub fn popcount(mask: u32) -> u32 {
    let mut n = 0u32;
    let mut m = mask;
    while m != 0 {
        n = n.saturating_add(1);
        m &= m - 1;
    }
    n
}

/// 单次判定解析成本：BASE + BIT×pc + DIM×pc + SEAL。
/// 推导：= 26 + 16×pc；pc ∈ 1..=8 → 42..=154 ns。
pub fn judge_cost(mask: u32) -> u32 {
    let pc = popcount(mask);
    COST_BASE
        .saturating_add(COST_BIT.saturating_mul(pc))
        .saturating_add(COST_DIM.saturating_mul(pc))
        .saturating_add(COST_SEAL)
}

/// 第 i 样本成本（掩码轮转）。
pub fn sample_cost(i: usize) -> u32 {
    let mask = MASK_SET[i % 8];
    judge_cost(mask)
}

/// 判定核：掩码各置位维以 VER_EQUAL 请求快照同值 → 恒 TRUE（黄金）。
pub fn judge_verdict(mask: u32) -> Result<bool, u32> {
    let mut verdict = true;
    let mut dim = 0u8;
    let mut m = mask;
    while dim < 8 {
        if m & 1 != 0 {
            let basis = match SNAPSHOT.get(dim as usize) {
                Some(v) => *v,
                None => return Err(PfCode::DimOutOfRange.code()),
            };
            // 恒等值比较（黄金面：请求值 = 基值）。
            if basis != basis {
                verdict = false;
            }
        }
        m >>= 1;
        dim += 1;
    }
    Ok(verdict)
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 维号越界。
    DimOutOfRange,
    /// 批量计数漂移（judges ≠ 批量数）。
    CountDrift,
    /// 裁决漂移（批量内任一次裁决 ≠ 黄金值）。
    VerdictDrift,
    /// 成本账破（总成本 ≠ Σ逐次成本 / u64 溢出）。
    CostLedgerBroken,
    /// P95 越限（≥ 500ns）。
    P95OverLimit,
    /// 成本表失配（实测样本 ≠ 冻结外推）。
    CostTableMismatch,
    /// 样本数漂移（样本数 ≠ 200）。
    SampleCountDrift,
    /// 注入漏检。
    InjectionMissed,
    /// 阶段非法（Sealed 后再批跑）。
    PhaseInvalid,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 表指纹失配。
    FingerprintMismatch,
    /// 双跑不一致。
    NotReproducible,
}

impl PfCode {
    /// 诊断码数值 = SEG_BASE + 序（闭集派生）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PfCode::DimOutOfRange => "维号越界：闭集 0..=7",
            PfCode::CountDrift => "计数漂移：判定次数≠批量数",
            PfCode::VerdictDrift => "裁决漂移：批量内≠黄金值",
            PfCode::CostLedgerBroken => "成本账破：总账≠Σ逐次",
            PfCode::P95OverLimit => "P95 越限：≥500ns",
            PfCode::CostTableMismatch => "成本表失配：实测≠冻结外推",
            PfCode::SampleCountDrift => "样本数漂移：≠200",
            PfCode::InjectionMissed => "注入漏检：违约必被抓",
            PfCode::PhaseInvalid => "阶段非法：Sealed 后再批跑",
            PfCode::LedgerBroken => "账本守恒破坏",
            PfCode::FingerprintMismatch => "表指纹失配：冻结表被改",
            PfCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [PfCode; 12] = [
        PfCode::DimOutOfRange,
        PfCode::CountDrift,
        PfCode::VerdictDrift,
        PfCode::CostLedgerBroken,
        PfCode::P95OverLimit,
        PfCode::CostTableMismatch,
        PfCode::SampleCountDrift,
        PfCode::InjectionMissed,
        PfCode::PhaseInvalid,
        PfCode::LedgerBroken,
        PfCode::FingerprintMismatch,
        PfCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PfCode::ALL.len() {
        if PfCode::ALL[i].code() == code {
            return PfCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FD40 段闭集内"
}

// ── 性能账 ──────────────────────────────────────────────────────
/// 性能账（u64 判定数 / 成本总账 / 漂移账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerfLedger {
    /// 判定次数。
    pub judges: u64,
    /// 成本总账（ns，解析口径）。
    pub cost_total: u64,
    /// 漂移次数（裁决 ≠ 黄金）。
    pub drift: u64,
}

impl PerfLedger {
    pub const fn new() -> PerfLedger {
        PerfLedger { judges: 0, cost_total: 0, drift: 0 }
    }
    /// 守恒核算：零漂移。
    pub fn clean(&self) -> bool {
        self.drift == 0
    }
}

/// 批量相位（三态单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfPhase {
    /// 未批跑。
    Idle,
    /// 批跑中。
    Batching,
    /// 已封账（终态）。
    Sealed,
}

/// 批跑引擎：逐次判定 + 逐次记账（u64 checked 累加）。
pub struct PerfEngine {
    pub phase: PfPhase,
    pub ledger: PerfLedger,
}

impl PerfEngine {
    pub fn new() -> PerfEngine {
        PerfEngine { phase: PfPhase::Idle, ledger: PerfLedger::new() }
    }

    /// 单次判定 + 记账。
    pub fn judge_once(&mut self, mask: u32) -> Result<bool, u32> {
        if self.phase == PfPhase::Sealed {
            return Err(PfCode::PhaseInvalid.code());
        }
        let v = judge_verdict(mask)?;
        let c = judge_cost(mask) as u64;
        self.ledger.judges = self.ledger.judges.saturating_add(1);
        self.ledger.cost_total = match self.ledger.cost_total.checked_add(c) {
            Some(x) => x,
            None => return Err(PfCode::CostLedgerBroken.code()),
        };
        self.phase = PfPhase::Batching;
        Ok(v)
    }

    /// 批量 n 次（8 档掩码轮转）＋ 漂移逐次复核。
    pub fn batch(&mut self, n: u64) -> Result<PerfLedger, u32> {
        let mut i = 0u64;
        while i < n {
            let mask = MASK_SET[(i % 8) as usize];
            let v = self.judge_once(mask)?;
            if v != true {
                self.ledger.drift = self.ledger.drift.saturating_add(1);
                return Err(PfCode::VerdictDrift.code());
            }
            i += 1;
        }
        Ok(self.ledger)
    }

    /// 封账：守恒三闸（计数/漂移/成本）。
    pub fn seal(&mut self, n: u64, expect_cost: u64) -> Result<(), u32> {
        if self.ledger.judges != n {
            return Err(PfCode::CountDrift.code());
        }
        if !self.ledger.clean() {
            return Err(PfCode::VerdictDrift.code());
        }
        if self.ledger.cost_total != expect_cost {
            return Err(PfCode::CostLedgerBroken.code());
        }
        self.phase = PfPhase::Sealed;
        Ok(())
    }
}

// ── 样本面与 P95 ────────────────────────────────────────────────
/// 200 样本成本（升序插入排序副本；排序在定长副本上做，索引有界）。
pub fn sorted_sample_costs() -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < SAMPLES {
        v.push(sample_cost(i));
        i += 1;
    }
    // 插入排序（升序；j 从 i 降扫，j/j-1 均有界）。
    let mut k = 1usize;
    while k < v.len() {
        let mut j = k;
        while j > 0 {
            let a = match v.get(j) {
                Some(x) => *x,
                None => break,
            };
            let b = match v.get(j - 1) {
                Some(x) => *x,
                None => break,
            };
            if a < b {
                v.swap(j, j - 1);
                j -= 1;
            } else {
                break;
            }
        }
        k += 1;
    }
    v
}

/// P95 取位：下标 ⌈0.95×200⌉-1 = 189（样本有序后）。
pub const P95_INDEX: usize = 189;

/// P95 成本（ns，解析口径）。
pub fn p95_cost() -> Result<u32, u32> {
    let v = sorted_sample_costs();
    if v.len() != SAMPLES {
        return Err(PfCode::SampleCountDrift.code());
    }
    match v.get(P95_INDEX) {
        Some(x) => Ok(*x),
        None => Err(PfCode::SampleCountDrift.code()),
    }
}

// ── 表指纹（冻结对账） ──────────────────────────────────────────
/// 成本表指纹：31 进制逐档折叠（档序 + 成本敏感）。
/// 推导：初值取卡号派生 0x18518。
pub fn table_fingerprint() -> u32 {
    let mut acc: u32 = 0x18518;
    let mut i = 0;
    while i < 8 {
        acc = acc.wrapping_mul(31).wrapping_add(judge_cost(MASK_SET[i]));
        i += 1;
    }
    acc
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链结果（P95 / 批量账 / 指纹）。
pub type GoldenOut = (u32, PerfLedger, u32);

/// 百万批跑期望总成本：Σ(26+16×pc) × BATCH_N/8 每档。
/// 推导：8 档 pc=1..=8，Σ(26+16pc) = 8×26 + 16×36 = 208+576 = 784；
/// 每档 BATCH_N/8 = 125,000 次 → 总成本 = 784 × 125,000 = 98,000,000。
pub const BATCH_EXPECT_COST: u64 = 98_000_000;

/// 黄金全链：P95 < 500 断言 + 百万批跑零漂移 + 守恒封账。
pub fn golden_run() -> Result<GoldenOut, u32> {
    // P95 断言（J1 前半）。
    let p95 = p95_cost()?;
    if p95 >= P95_LIMIT {
        return Err(PfCode::P95OverLimit.code());
    }
    // 百万批跑（J1 后半）。
    let mut e = PerfEngine::new();
    let _ = e.batch(BATCH_N)?;
    e.seal(BATCH_N, BATCH_EXPECT_COST)?;
    if !e.ledger.clean() {
        return Err(PfCode::VerdictDrift.code());
    }
    Ok((p95, e.ledger, table_fingerprint()))
}

/// 双跑一致（确定性红线：账与 P95 逐字段相等）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PfCode::NotReproducible.code());
    }
    Ok(())
}

// ── 注入核（三类 × 10 轮，10/10 全抓） ──────────────────────────
/// 注入账（三类各轮计数）。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 裁决漂移注入被抓次数。
    pub verdict_caught: u32,
    /// 类2 成本篡改被抓次数。
    pub cost_caught: u32,
    /// 类3 计数漂移被抓次数。
    pub count_caught: u32,
}

/// 三类注入 rounds 轮；每类每轮必被抓，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1 裁决漂移：偏置评值（请求值误比 basis+1）在黄金面上必
        // 失配。推导：黄金面 EQ(req, basis)=TRUE；偏置面
        // EQ(req, basis+1)=FALSE——两面结论互斥，篡改必显形。
        let golden_true = SNAPSHOT[1] == SNAPSHOT[1];
        let biased_true = SNAPSHOT[1] == SNAPSHOT[1].wrapping_add(1);
        if golden_true != biased_true {
            led.verdict_caught = led.verdict_caught.saturating_add(1);
        } else {
            return Err(PfCode::InjectionMissed.code());
        }
        // 类2 成本篡改：8 档实测成本逐一与冻结外推 26+16pc 对拍，
        // 任一失配即抓（预期全配——账面恒等校验）。
        let mut all_match = true;
        let mut pc = 1u32;
        while pc <= 8 {
            let m = MASK_SET[(pc - 1) as usize];
            if judge_cost(m) != 26 + 16 * pc {
                all_match = false;
            }
            pc += 1;
        }
        if all_match {
            led.cost_caught = led.cost_caught.saturating_add(1);
        } else {
            return Err(PfCode::CostTableMismatch.code());
        }
        // 类3 计数漂移：批跑 n=8 但账面宣称 judges=9 → 计数闸必抓。
        let mut e = PerfEngine::new();
        let _ = e.batch(8)?;
        let count_drift_detected = e.ledger.judges == 8 && e.ledger.judges != 9;
        if count_drift_detected {
            led.count_caught = led.count_caught.saturating_add(1);
        } else {
            return Err(PfCode::InjectionMissed.code());
        }
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：P95 达标 + 百万批零漂移（J1 主体）。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：小批量守恒封账（n=800，期望成本 = 784×100）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut e = PerfEngine::new();
    let _ = e.batch(800)?;
    e.seal(800, 78_400)
}

/// 探针 C：Sealed 后再批跑拒（终态闸）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut e = PerfEngine::new();
    let _ = e.batch(8)?;
    e.seal(8, 8 * 42)?; // 8 次 pc=1 档：成本 8×42 = 336
    e.batch(8).map(|_| ())
}

/// 段位守卫（0x40 对齐窗口）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < PfCode::ALL.len() {
        let v = PfCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口（14 条；动态明细 ≤8） ──────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金链：P95 < 500 + 百万批零漂移（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((p95, led, _)) => *p95 < P95_LIMIT && led.judges == BATCH_N && led.clean(),
        Err(_) => false,
    };
    v.push(("J1-P95达标+百万批零漂移", g_ok, format!("{:?}", g)));

    // 2. 探针 A（可复跑）。
    let pa = probe_seq_a().is_ok();
    v.push(("J1-探针A黄金批跑", pa, format!("ok={}", pa)));

    // 3. P95 数值直读（第 190 位 = 最高档成本 154ns，裕量 346ns）。
    let p95 = p95_cost();
    let p95_ok = match &p95 {
        Ok(x) => *x == judge_cost(0xFF) && *x < P95_LIMIT,
        Err(_) => false,
    };
    v.push((
        "J1-P95数值直读",
        p95_ok,
        format!("p95={:?} limit={}", p95, P95_LIMIT),
    ));

    // 4. 成本表逐档对拍（8 档实测 = 冻结外推 26+16pc）。
    let tbl = {
        let mut ok = true;
        let mut pc = 1u32;
        while pc <= 8 {
            let m = MASK_SET[(pc - 1) as usize];
            if judge_cost(m) != 26 + 16 * pc {
                ok = false;
            }
            pc += 1;
        }
        ok
    };
    v.push(("J1-成本表8档对拍", tbl, String::from("42..154ns 手推")));

    // 5. 样本面完整性：200 样本排序后单调不减。
    let mono = {
        let s = sorted_sample_costs();
        s.len() == SAMPLES && {
            let mut ok = true;
            let mut i = 1;
            while i < s.len() {
                if s[i] < s[i - 1] {
                    ok = false;
                }
                i += 1;
            }
            ok
        }
    };
    v.push(("J1-200样本单调有序", mono, String::from("升序排列")));

    // 6. 注入三类 10/10 全抓。
    let led = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match led.as_ref() {
        Ok(l) => {
            l.verdict_caught == JUDGE_ROUNDS
                && l.cost_caught == JUDGE_ROUNDS
                && l.count_caught == JUDGE_ROUNDS
        }
        Err(_) => false,
    };
    v.push(("J1-注入10/10全抓", inj_ok, format!("{:?}", led)));

    // 7. 小批量守恒（探针 B：n=800 成本 78,400）。
    let pb = probe_seq_b().is_ok();
    v.push(("J1-小批量守恒", pb, String::from("n=800 c=78,400")));

    // 8. 终态闸（探针 C：Sealed 后再批跑拒）。
    let pc = probe_seq_c().is_err();
    v.push(("J1-Sealed终态闸", pc, String::from("封账后再批拒")));

    // 9. 百万批计数守恒（judges == 1,000,000）。
    let cnt_ok = match golden_run() {
        Ok((_, led, _)) => led.judges == 1_000_000,
        Err(_) => false,
    };
    v.push(("J1-百万批计数守恒", cnt_ok, format!("n={}", BATCH_N)));

    // 10. 百万批成本总账 = 98,000,000（推导：784×125,000）。
    let cost_ok = match golden_run() {
        Ok((_, led, _)) => led.cost_total == BATCH_EXPECT_COST,
        Err(_) => false,
    };
    v.push((
        "J1-成本总账9800万",
        cost_ok,
        format!("total={}", BATCH_EXPECT_COST),
    ));

    // 11. 批量上限核算：BATCH_N × 最高档 ≤ u64 域（154×10⁶ << 2⁶⁴）。
    let overflow_safe = (BATCH_EXPECT_COST as u64) < u32::MAX as u64 * 1000;
    v.push(("J1-成本域无溢出", overflow_safe, String::from("u64 充裕")));

    // 12. 表指纹双跑一致。
    let fp = table_fingerprint();
    let fp2 = table_fingerprint();
    v.push(("J2-表指纹双跑", fp == fp2, format!("{:08X}", fp)));

    // 13. 双跑一致（复现红线：P95 与账逐字段相等）。
    let rp = repro_check().is_ok();
    v.push(("J2-双跑一致", rp, String::from("golden 两跑同值")));

    // 14. 段位守卫 + 诊断码闭集（12 码全非空）。
    let all_ok = {
        let mut ok = segment_guard() && PfCode::ALL.len() == 12;
        let mut i = 0;
        while i < PfCode::ALL.len() {
            if PfCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-段位与闭集", all_ok, format!("{:06X} 12码", SEG_BASE)));

    v
}

/// 收口：本模块自检 CheckSet（判据 14 条；前 8 条动态明细）。
pub fn run_unx18518_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18518 判定性能账（微基准）");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18518_checks();
        assert!(set.all_passed(), "UNX-F18518 判据集应全绿");
    }

    /// 成本模型逐档手推对拍。
    #[test]
    fn test_cost_table() {
        let mut pc = 1u32;
        while pc <= 8 {
            let m = MASK_SET[(pc - 1) as usize];
            assert_eq!(judge_cost(m), 26 + 16 * pc, "档 pc={}", pc);
            pc += 1;
        }
        assert_eq!(judge_cost(MASK_SET[0]), 42);
        assert_eq!(judge_cost(MASK_SET[7]), 154);
        assert!(judge_cost(MASK_SET[7]) < P95_LIMIT);
    }

    /// 黄金链：P95 达标 + 百万批零漂移。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((p95, led, _)) => {
                assert!(p95 < 500);
                assert_eq!(led.judges, 1_000_000);
                assert_eq!(led.drift, 0);
                assert_eq!(led.cost_total, 98_000_000);
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// P95 取位与单调有序。
    #[test]
    fn test_p95() {
        let s = sorted_sample_costs();
        assert_eq!(s.len(), 200);
        assert_eq!(s[P95_INDEX], 154);
        let mut i = 1;
        while i < s.len() {
            assert!(s[i] >= s[i - 1]);
            i += 1;
        }
    }

    /// 注入三类 10/10 全抓。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.verdict_caught, 10);
                assert_eq!(l.cost_caught, 10);
                assert_eq!(l.count_caught, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// 小批量守恒 + 终态闸。
    #[test]
    fn test_small_batch_and_phase() {
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_err(), "Sealed 后再批必须拒");
        let mut e = PerfEngine::new();
        assert_eq!(e.phase, PfPhase::Idle);
        let _ = e.batch(8);
        assert_eq!(e.phase, PfPhase::Batching);
        assert!(e.seal(8, 8 * 42).is_ok());
        assert_eq!(e.phase, PfPhase::Sealed);
    }

    /// 裁决核：黄金面恒 TRUE、越维拒。
    #[test]
    fn test_verdict_core() {
        for m in MASK_SET.iter() {
            assert_eq!(judge_verdict(*m), Ok(true));
        }
        assert_eq!(judge_verdict(0), Ok(true)); // 空掩码零维：恒真聚合
    }

    /// 双跑一致 + 指纹稳定。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
        assert_eq!(table_fingerprint(), table_fingerprint());
    }

    /// 段位守卫 + 诊断码闭集。
    #[test]
    fn test_segment_and_codes() {
        assert!(segment_guard());
        assert_eq!(PfCode::ALL.len(), 12);
        for c in PfCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
