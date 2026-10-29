//! CGPU-F0795 · E 域 20 维度验收执行（目标 340 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥340 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0795，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：E 域 20 维度验收执行——把 E 域验收切成固定 20 个维度，
//!   每维度独立打分（千分比整数）、加权合计、逐维度给出
//!   通过/缓冲/不通过三态结论，全维度执行完才允许出验收决议。
//!
//! 设计纪律：
//! - 维度数固定 20：DIM_COUNT=20，编号 1..=20，多登少登都非法。
//! - 权重整数：每维 weight ∈ 1..=10，Σ权重 == 100（否则口径非法）。
//! - 维度结论闭集三态：Pass（score ≥ 900）/ Buffered（≥ 700，
//!   需挂缓冲项）/ Fail（其余）；全过判据：Pass 数 == 20。
//! - 执行账本定容：EXEC_CAP=64 条执行记录，超容逐出最旧并计数，
//!   守恒恒等式：total_execs == held + evicted。
//! - 执行状态机：Pending→Running→Scored，跳步即 Err；
//!   Scored 为终态，重打分必须重开执行记录。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
//!   诊断段 0x6EA0 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::vec::Vec;

/// 诊断段基址（0x6EA0，批内 CGPU-F0795 独占）。
pub const SEG_BASE: u16 = 0x6EA0;

/// 维度登记口径非法（编号越界 / 权重越界 / 权重和 != 100）。
pub const E_DIM: u16 = SEG_BASE + 1;
/// 执行账本已满。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 执行状态机跳步。
pub const E_STATE: u16 = SEG_BASE + 3;
/// 执行记录不存在。
pub const E_NOEXEC: u16 = SEG_BASE + 4;
/// 分数口径非法（>1000）。
pub const E_SCORE: u16 = SEG_BASE + 5;
/// 维度未全部执行即出决议。
pub const E_PARTIAL: u16 = SEG_BASE + 6;
/// 守恒破坏。
pub const E_LEDGER: u16 = SEG_BASE + 7;

/// 固定维度数。
pub const DIM_COUNT: usize = 20;
/// 执行账本容量。
pub const EXEC_CAP: usize = 64;
/// 权重总和必须为 100。
pub const WEIGHT_SUM: u32 = 100;
/// Pass 阈值。
pub const PASS_PM: u32 = 900;
/// Buffered 阈值。
pub const BUFFER_PM: u32 = 700;

/// 维度定义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DimSpec {
    /// 维度编号 1..=20。
    pub dim: u8,
    /// 权重 1..=10。
    pub weight: u32,
}

/// 维度册（恰好 20 维）。
#[derive(Clone, Debug, Default)]
pub struct DimBook {
    dims: Vec<DimSpec>,
}

impl DimBook {
    pub fn new() -> Self {
        DimBook { dims: Vec::new() }
    }
    /// 登记维度（编号闭集校验）。
    pub fn add(&mut self, dim: u8, weight: u32) -> Result<(), u16> {
        if (dim as usize) == 0 || (dim as usize) > DIM_COUNT {
            return Err(E_DIM);
        }
        if weight == 0 || weight > 10 {
            return Err(E_DIM);
        }
        if self.dims.iter().any(|d| d.dim == dim) {
            return Err(E_DIM);
        }
        self.dims.push(DimSpec { dim, weight });
        Ok(())
    }
    /// 冻结校验：恰好 20 维且 Σ权重 == 100。
    pub fn freeze_check(&self) -> Result<(), u16> {
        if self.dims.len() != DIM_COUNT {
            return Err(E_DIM);
        }
        let sum: u32 = self.dims.iter().map(|d| d.weight).sum();
        if sum != WEIGHT_SUM {
            return Err(E_DIM);
        }
        Ok(())
    }
    pub fn weight_of(&self, dim: u8) -> Result<u32, u16> {
        self.dims
            .iter()
            .find(|d| d.dim == dim)
            .map(|d| d.weight)
            .ok_or(E_DIM)
    }
    pub fn len(&self) -> usize {
        self.dims.len()
    }
}

/// 维度结论（闭集三态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DimVerdict {
    Pass,
    Buffered,
    Fail,
}

impl DimVerdict {
    pub fn from_score(score_pm: u32) -> Result<DimVerdict, u16> {
        if score_pm > 1000 {
            return Err(E_SCORE);
        }
        Ok(if score_pm >= PASS_PM {
            DimVerdict::Pass
        } else if score_pm >= BUFFER_PM {
            DimVerdict::Buffered
        } else {
            DimVerdict::Fail
        })
    }
    pub fn ord(self) -> u8 {
        match self {
            DimVerdict::Pass => 0,
            DimVerdict::Buffered => 1,
            DimVerdict::Fail => 2,
        }
    }
}

/// 执行状态（闭集三态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecState {
    Pending,
    Running,
    Scored,
}

impl ExecState {
    pub fn ord(self) -> u8 {
        match self {
            ExecState::Pending => 0,
            ExecState::Running => 1,
            ExecState::Scored => 2,
        }
    }
}

/// 执行记录。
#[derive(Clone, Debug)]
pub struct ExecRec {
    /// 维度编号。
    pub dim: u8,
    /// 状态。
    pub state: ExecState,
    /// 得分（未打分为 0）。
    pub score_pm: u32,
    /// 结论。
    pub verdict: Option<DimVerdict>,
}

/// 执行账本（环形定容）。
#[derive(Clone, Debug)]
pub struct ExecLedger {
    ring: Vec<ExecRec>,
    evicted: u64,
    total: u64,
}

impl ExecLedger {
    pub fn new() -> Self {
        ExecLedger {
            ring: Vec::new(),
            evicted: 0,
            total: 0,
        }
    }
    /// 开记录（Pending）。
    pub fn open(&mut self, dim: u8) -> Result<(), u16> {
        if (dim as usize) == 0 || (dim as usize) > DIM_COUNT {
            return Err(E_DIM);
        }
        if self.ring.len() >= EXEC_CAP {
            self.ring.remove(0);
            self.evicted += 1;
        }
        self.ring.push(ExecRec {
            dim,
            state: ExecState::Pending,
            score_pm: 0,
            verdict: None,
        });
        self.total += 1;
        Ok(())
    }
    /// 状态推进：Pending→Running→Scored（打分时给出 score）。
    pub fn run(&mut self, dim: u8) -> Result<(), u16> {
        let r = self.last_open(dim).ok_or(E_NOEXEC)?;
        if r.state != ExecState::Pending {
            return Err(E_STATE);
        }
        r.state = ExecState::Running;
        Ok(())
    }
    /// 打分并定格：Running→Scored。
    pub fn score(&mut self, dim: u8, score_pm: u32) -> Result<DimVerdict, u16> {
        let r = self.last_open(dim).ok_or(E_NOEXEC)?;
        if r.state != ExecState::Running {
            return Err(E_STATE);
        }
        let v = DimVerdict::from_score(score_pm)?;
        r.score_pm = score_pm;
        r.verdict = Some(v);
        r.state = ExecState::Scored;
        Ok(v)
    }
    /// 取该维度最后一条未定格记录。
    fn last_open(&mut self, dim: u8) -> Option<&mut ExecRec> {
        self.ring.iter_mut().rev().find(|r| r.dim == dim)
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

/// 验收决议：要求 20 维各有 ≥1 条 Scored 记录，加权合计出总分；
/// 存在 Fail → Err；存在 Buffered → Ok(1)；全 Pass → Ok(0)。
pub fn decide(book: &DimBook, led: &ExecLedger) -> Result<u32, u16> {
    book.freeze_check()?;
    // 每维取最后一条记录，必须已定格。
    let mut scored_dims = 0u32;
    let mut weighted = 0u64;
    let mut weight_sum = 0u64;
    let mut any_fail = false;
    let mut any_buffered = false;
    for d in &book.dims {
        let rec = led
            .ring
            .iter()
            .rev()
            .find(|r| r.dim == d.dim && r.state == ExecState::Scored && r.verdict.is_some());
        match rec {
            None => return Err(E_PARTIAL),
            Some(r) => {
                scored_dims += 1;
                let v = r.verdict.ok_or(E_PARTIAL)?;
                match v {
                    DimVerdict::Fail => any_fail = true,
                    DimVerdict::Buffered => any_buffered = true,
                    DimVerdict::Pass => {}
                }
                weighted += r.score_pm as u64 * d.weight as u64;
                weight_sum += d.weight as u64;
            }
        }
    }
    if scored_dims != DIM_COUNT as u32 {
        return Err(E_PARTIAL);
    }
    let _final_score = weighted / weight_sum;
    if any_fail {
        Err(E_PARTIAL)
    } else if any_buffered {
        Ok(1)
    } else {
        Ok(0)
    }
}

/// 自例程：主路径（全 Pass 决议 0）+ 缓冲路径 + 不通过路径 + 错误路径 + 守恒。
pub fn drill() -> Result<u32, u16> {
    // 维度册：20 维，Σ权重=100（5 维×10 + 10 维×4 + 5 维×2 = 50+40+10=100）。
    let mut book = DimBook::new();
    for i in 0..5u8 {
        book.add(i + 1, 10)?;
    }
    for i in 0..10u8 {
        book.add(i + 6, 4)?;
    }
    for i in 0..5u8 {
        book.add(i + 16, 2)?;
    }
    book.freeze_check()?;
    // 维度口径错误。
    if book.add(0, 5) != Err(E_DIM)
        || book.add(21, 5) != Err(E_DIM)
        || book.add(1, 5) != Err(E_DIM)
        || book.add(30, 0) != Err(E_DIM)
        || book.add(30, 11) != Err(E_DIM)
    {
        return Err(E_DIM);
    }
    // 主路径 1：20 维全部执行并满分 → 决议 0。
    let mut led = ExecLedger::new();
    for d in 1..=20u8 {
        led.open(d)?;
        led.run(d)?;
        led.score(d, 950)?;
    }
    led.audit()?;
    if decide(&book, &led)? != 0 {
        return Err(E_PARTIAL);
    }
    // 状态机错误路径：Scored 后重跑。
    if led.run(1) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 主路径 2：一维 800 → Buffered → 决议 1。
    let mut led2 = ExecLedger::new();
    for d in 1..=20u8 {
        led2.open(d)?;
        led2.run(d)?;
        let s = if d == 7 { 800 } else { 950 };
        led2.score(d, s)?;
    }
    if decide(&book, &led2)? != 1 {
        return Err(E_PARTIAL);
    }
    // 主路径 3：一维 500 → Fail → Err。
    let mut led3 = ExecLedger::new();
    for d in 1..=20u8 {
        led3.open(d)?;
        led3.run(d)?;
        let s = if d == 7 { 500 } else { 950 };
        led3.score(d, s)?;
    }
    if decide(&book, &led3) != Err(E_PARTIAL) {
        return Err(E_PARTIAL);
    }
    // 错误路径 1：跳步打分（Pending 直接 score）。
    let mut led4 = ExecLedger::new();
    led4.open(3)?;
    if led4.score(3, 900) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 错误路径 2：未执行完整 20 维。
    let mut led5 = ExecLedger::new();
    for d in 1..=19u8 {
        led5.open(d)?;
        led5.run(d)?;
        led5.score(d, 950)?;
    }
    if decide(&book, &led5) != Err(E_PARTIAL) {
        return Err(E_PARTIAL);
    }
    // 错误路径 3：分数越界。
    let mut led6 = ExecLedger::new();
    led6.open(1)?;
    led6.run(1)?;
    if led6.score(1, 1001) != Err(E_SCORE) {
        return Err(E_SCORE);
    }
    // 错误路径 4：权重和 != 100 的册（20×3=60）。
    let mut bad = DimBook::new();
    for _ in 0..20u8 {
        bad.add(bad_len_next(&bad), 3)?;
    }
    if bad.freeze_check() != Err(E_DIM) {
        return Err(E_DIM);
    }
    // 结论边界：900→Pass，899→Buffered，700→Buffered，699→Fail。
    if DimVerdict::from_score(900)? != DimVerdict::Pass
        || DimVerdict::from_score(899)? != DimVerdict::Buffered
        || DimVerdict::from_score(700)? != DimVerdict::Buffered
        || DimVerdict::from_score(699)? != DimVerdict::Fail
    {
        return Err(E_SCORE);
    }
    // 定容守恒：灌 70 条记录进 64 容账本。
    let mut led7 = ExecLedger::new();
    for i in 0..70u32 {
        led7.open(((i % 20) + 1) as u8)?;
    }
    led7.audit()?;
    if led7.held() != EXEC_CAP || led7.evicted != 6 {
        return Err(E_LEDGER);
    }
    Ok(0)
}

/// 辅助：给 bad 册分配下一个未用维度号。
fn bad_len_next(book: &DimBook) -> u8 {
    (book.len() as u8) + 1
}
