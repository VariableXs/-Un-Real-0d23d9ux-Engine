//! CGPU-F0789 · E 域性能总基准（目标 340 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥340 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0789，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：E 域性能总基准——把 E 域全部性能口径收敛为一张总基准表：
//!   场景（Scenario）× 预算（Budget）× 观测（Observation），
//!   整数微秒计时，逐项判达标，总分千分比加权，档位闭集裁定。
//!
//! 设计纪律：
//! - 基准项闭集四类：Raster（光栅吞吐）、Blend（混合带宽）、
//!   Compile（管线编译时长）、Wake（唤醒延迟）——无第五类。
//! - 预算口径：每项给整数预算（单位：微秒/千操作），观测超预算
//!   1.5×内记 SLIP（滑档但过），超 1.5×记 MISS（不过）；无小数。
//! - 计分：达标=满权重；SLIP=半权重；MISS=0；总分=Σ得分/Σ权重
//!   ×1000（整数除法），下限 0。
//! - 结果账本定容：RUN_CAP=96 条运行记录，超容逐出最旧并计数，
//!   守恒恒等式：total_runs == held + evicted。
//! - 档位闭集四档：Gold(≥950)/Silver(≥850)/Bronze(≥700)/Fail(其余)。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
//!   诊断段 0x6DE0 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x6DE0，批内 CGPU-F0789 独占）。
pub const SEG_BASE: u16 = 0x6DE0;

/// 场景编号重复登记。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 基准册已满（防御位）。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 观测序列与基准册条目数不一致。
pub const E_MISMATCH: u16 = SEG_BASE + 3;
/// 预算或权重口径非法（0）。
pub const E_BUDGET: u16 = SEG_BASE + 4;
/// 无基准项即跑总基准。
pub const E_EMPTY: u16 = SEG_BASE + 5;
/// 运行记录账本守恒破坏。
pub const E_LEDGER: u16 = SEG_BASE + 6;

/// 运行记录账本容量。
pub const RUN_CAP: usize = 96;
/// SLIP 判定分母（观测 ≤ 预算×3/2 记 SLIP，整数 3/2 口径）。
pub const SLIP_NUM: u64 = 3;
pub const SLIP_DEN: u64 = 2;
/// 档位阈值（千分比）。
pub const GOLD_PM: u32 = 950;
pub const SILVER_PM: u32 = 850;
pub const BRONZE_PM: u32 = 700;

/// 基准项类别（闭集四类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchKind {
    /// 光栅吞吐（微秒/千操作，越低越好）。
    Raster,
    /// 混合带宽（微秒/千操作，越低越好）。
    Blend,
    /// 管线编译时长（微秒，越低越好）。
    Compile,
    /// 唤醒延迟（微秒，越低越好）。
    Wake,
}

impl BenchKind {
    pub fn ord(self) -> u8 {
        match self {
            BenchKind::Raster => 0,
            BenchKind::Blend => 1,
            BenchKind::Compile => 2,
            BenchKind::Wake => 3,
        }
    }
}

/// 单项判定（闭集三态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemVerdict {
    /// 观测 ≤ 预算：达标。
    Hit,
    /// 观测 ≤ 预算×1.5：滑档但过。
    Slip,
    /// 超预算×1.5：不过。
    Miss,
}

/// 基准项。
#[derive(Clone, Debug)]
pub struct BenchItem {
    /// 场景名。
    pub scene: &'static str,
    /// 类别。
    pub kind: BenchKind,
    /// 预算（微秒或微秒/千操作）。
    pub budget_us: u64,
    /// 权重（1..=10）。
    pub weight: u32,
}

impl BenchItem {
    pub fn new(scene: &'static str, kind: BenchKind, budget_us: u64, weight: u32) -> Result<Self, u16> {
        if budget_us == 0 || weight == 0 || weight > 10 {
            return Err(E_BUDGET);
        }
        Ok(BenchItem {
            scene,
            kind,
            budget_us,
            weight,
        })
    }
    /// 单项判定：整数 1.5× 口径（budget*3/2）。
    pub fn judge(&self, observed_us: u64) -> ItemVerdict {
        if observed_us <= self.budget_us {
            ItemVerdict::Hit
        } else if observed_us <= self.budget_us * SLIP_NUM / SLIP_DEN {
            ItemVerdict::Slip
        } else {
            ItemVerdict::Miss
        }
    }
    /// 单项得分（千分比折算分子）：Hit=权重，Slip=权重/2，Miss=0。
    pub fn score(&self, observed_us: u64) -> u64 {
        match self.judge(observed_us) {
            ItemVerdict::Hit => self.weight as u64,
            ItemVerdict::Slip => self.weight as u64 / 2,
            ItemVerdict::Miss => 0,
        }
    }
}

/// 总基准册。
#[derive(Clone, Debug, Default)]
pub struct BenchBook {
    items: Vec<BenchItem>,
}

impl BenchBook {
    pub fn new() -> Self {
        BenchBook { items: Vec::new() }
    }
    /// 登记基准项（场景名唯一）。
    pub fn register(&mut self, it: BenchItem) -> Result<(), u16> {
        if self.items.iter().any(|o| o.scene == it.scene) {
            return Err(E_DUP);
        }
        self.items.push(it);
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    /// 总权重（计分分母）。
    pub fn total_weight(&self) -> u64 {
        self.items.iter().map(|i| i.weight as u64).sum()
    }
}

/// 运行记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunRec {
    /// 场景名（闭集引用，基准册内必须存在）。
    pub scene: &'static str,
    /// 观测值（微秒口径）。
    pub observed_us: u64,
    /// 判定。
    pub verdict: ItemVerdict,
}

/// 运行记录账本（环形定容）。
#[derive(Clone, Debug)]
pub struct RunLedger {
    ring: Vec<RunRec>,
    evicted: u64,
    total: u64,
}

impl RunLedger {
    pub fn new() -> Self {
        RunLedger {
            ring: Vec::new(),
            evicted: 0,
            total: 0,
        }
    }
    pub fn push(&mut self, r: RunRec) {
        if self.ring.len() >= RUN_CAP {
            self.ring.remove(0);
            self.evicted += 1;
        }
        self.ring.push(r);
        self.total += 1;
    }
    /// 守恒审计：total == held + evicted。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total != self.ring.len() as u64 + self.evicted {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    pub fn held(&self) -> usize {
        self.ring.len()
    }
}

/// 总基准档位（闭集四档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchTier {
    Gold,
    Silver,
    Bronze,
    Fail,
}

impl BenchTier {
    pub fn from_score(score_pm: u32) -> BenchTier {
        if score_pm >= GOLD_PM {
            BenchTier::Gold
        } else if score_pm >= SILVER_PM {
            BenchTier::Silver
        } else if score_pm >= BRONZE_PM {
            BenchTier::Bronze
        } else {
            BenchTier::Fail
        }
    }
}

/// 基准报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchReport {
    /// 总分（千分比）。
    pub score_pm: u32,
    /// 档位。
    pub tier: BenchTier,
    /// 达标项数。
    pub hits: u32,
    /// 滑档项数。
    pub slips: u32,
    /// 不过项数。
    pub misses: u32,
}

impl BenchReport {
    /// 守恒：hits+slips+misses == 总项数。
    pub fn audit(&self, items: usize) -> Result<(), u16> {
        if (self.hits + self.slips + self.misses) as usize != items {
            return Err(E_MISMATCH);
        }
        Ok(())
    }
}

/// 跑总基准：观测序列与册内条目按序对应。
pub fn run_master(book: &BenchBook, observed: &[u64], led: &mut RunLedger) -> Result<BenchReport, u16> {
    if book.is_empty() {
        return Err(E_EMPTY);
    }
    if observed.len() != book.len() {
        return Err(E_MISMATCH);
    }
    let mut hits = 0u32;
    let mut slips = 0u32;
    let mut misses = 0u32;
    let mut gained = 0u64;
    for (i, it) in book.items.iter().enumerate() {
        let v = it.judge(observed[i]);
        led.push(RunRec {
            scene: it.scene,
            observed_us: observed[i],
            verdict: v,
        });
        match v {
            ItemVerdict::Hit => hits += 1,
            ItemVerdict::Slip => slips += 1,
            ItemVerdict::Miss => misses += 1,
        }
        gained += it.score(observed[i]);
    }
    let denom = book.total_weight();
    let score = (gained * 1000 / denom).min(1000) as u32;
    Ok(BenchReport {
        score_pm: score,
        tier: BenchTier::from_score(score),
        hits,
        slips,
        misses,
    })
}

/// 自例程：主路径（全达标 Gold）+ 滑档路径 + 不过路径 + 错误路径 + 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut book = BenchBook::new();
    book.register(BenchItem::new("tri_raster", BenchKind::Raster, 1000, 4))?;
    book.register(BenchItem::new("alpha_blend", BenchKind::Blend, 800, 3))?;
    book.register(BenchItem::new("pipe_compile", BenchKind::Compile, 5000, 2))?;
    book.register(BenchItem::new("cold_wake", BenchKind::Wake, 300, 1))?;
    if book.register(BenchItem::new("tri_raster", BenchKind::Blend, 1, 1)) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if BenchItem::new("bad", BenchKind::Raster, 0, 1) != Err(E_BUDGET)
        || BenchItem::new("bad", BenchKind::Raster, 10, 0) != Err(E_BUDGET)
        || BenchItem::new("bad", BenchKind::Raster, 10, 11) != Err(E_BUDGET)
    {
        return Err(E_BUDGET);
    }
    // 主路径 1：全部达标 → 满分 1000 → Gold。
    let mut led = RunLedger::new();
    let obs_good: [u64; 4] = [900, 700, 4800, 250];
    let r1 = run_master(&book, &obs_good, &mut led)?;
    r1.audit(book.len())?;
    if r1.score_pm != 1000 || r1.tier != BenchTier::Gold || r1.hits != 4 {
        return Err(E_EMPTY);
    }
    led.audit()?;
    // 主路径 2：全部滑档 → 半分 500 → Fail（700 之下）。
    let mut led2 = RunLedger::new();
    let obs_slip: [u64; 4] = [1400, 1100, 7000, 420];
    let r2 = run_master(&book, &obs_slip, &mut led2)?;
    r2.audit(book.len())?;
    if r2.slips != 4 || r2.tier != BenchTier::Fail {
        return Err(E_EMPTY);
    }
    // 主路径 3：混合 → 精确分数核算：weights=10；Hit 项 4+3+1=8，Slip 项 2/2=1 → 9/10*1000=900 → Silver。
    let mut led3 = RunLedger::new();
    let obs_mix: [u64; 4] = [1000, 800, 7000, 300];
    let r3 = run_master(&book, &obs_mix, &mut led3)?;
    r3.audit(book.len())?;
    if r3.score_pm != 900 || r3.tier != BenchTier::Silver {
        return Err(E_EMPTY);
    }
    // 边界口径：观测=预算 → Hit；观测=预算*3/2 → Slip；+1 → Miss。
    let it = BenchItem::new("edge", BenchKind::Raster, 100, 5)?;
    if it.judge(100) != ItemVerdict::Hit || it.judge(150) != ItemVerdict::Slip || it.judge(151) != ItemVerdict::Miss {
        return Err(E_BUDGET);
    }
    // 档位边界：950→Gold，949→Silver，850→Silver，849→Bronze，700→Bronze，699→Fail。
    if BenchTier::from_score(950) != BenchTier::Gold
        || BenchTier::from_score(949) != BenchTier::Silver
        || BenchTier::from_score(850) != BenchTier::Silver
        || BenchTier::from_score(849) != BenchTier::Bronze
        || BenchTier::from_score(700) != BenchTier::Bronze
        || BenchTier::from_score(699) != BenchTier::Fail
    {
        return Err(E_EMPTY);
    }
    // 错误路径 1：观测长度不一致。
    if run_master(&book, &[1, 2], &mut led) != Err(E_MISMATCH) {
        return Err(E_MISMATCH);
    }
    // 错误路径 2：空册。
    let empty = BenchBook::new();
    if run_master(&empty, &[], &mut led) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 定容守恒：灌 100 条记录进 96 容账本，恒等式仍成立。
    let mut led4 = RunLedger::new();
    for i in 0..100u32 {
        led4.push(RunRec {
            scene: "fill",
            observed_us: i as u64,
            verdict: ItemVerdict::Hit,
        });
    }
    led4.audit()?;
    if led4.held() != RUN_CAP || led4.evicted != 4 {
        return Err(E_LEDGER);
    }
    Ok(0)
}
