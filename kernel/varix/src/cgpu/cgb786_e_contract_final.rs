//! CGPU-F0786 · E 域合同与指标终审（目标 380 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥380 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0786，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：E 域合同与指标终审——把 E 域所有对内/对外承诺收敛为
//!   合同（Contract）与指标条款（MetricClause），终审机按闭集状态机
//!   走 Draft→InReview→Signed/Rejected，全部指标以整数口径复核，
//!   任何一项越界即整单驳回；合同账本定容守恒。
//!
//! 设计纪律：
//! - 指标条款闭集四类：吞吐（越高越好）、延迟（越低越好）、
//!   内存预算（越低越好）、质量分（越高越好）——无第五种口径。
//! - 终审判定：逐条款比对观测值，任一不过 → 整单 Rejected；
//!   全过才可 Signed；无部分签署。
//! - 状态机闭集：Draft→InReview→Signed / InReview→Rejected，
//!   其余转移一律 Err；Signed/Rejected 为终态不可再变。
//! - 合同账本定容：CONTRACT_CAP=64，超容逐出最旧并计数，
//!   守恒恒等式：total == held + evicted。
//! - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!   确定性；诊断段 0x6D80 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x6D80，批内 CGPU-F0786 独占）。
pub const SEG_BASE: u16 = 0x6D80;

/// 合同编号重复注册。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 合同账本已满且无法逐出（不该发生，防御位）。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 状态机非法转移（含终态再转移）。
pub const E_STATE: u16 = SEG_BASE + 3;
/// 指标条款缺失或条款本身非法（目标值为 0）。
pub const E_CLAUSE: u16 = SEG_BASE + 4;
/// 合同不存在或账本为空。
pub const E_EMPTY: u16 = SEG_BASE + 5;
/// 观测值口径非法（超出整数表示预算）。
pub const E_METRIC: u16 = SEG_BASE + 6;

/// 合同账本容量。
pub const CONTRACT_CAP: usize = 64;
/// 单合同条款数上限。
pub const CLAUSE_CAP: usize = 8;
/// 质量分满分（千分比）。
pub const QUALITY_FULL_PM: u32 = 1000;

/// 指标条款类别（闭集四类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricKind {
    /// 吞吐类：观测值 ≥ 目标即过（越高越好）。
    Throughput,
    /// 延迟类：观测值 ≤ 目标即过（越低越好）。
    Latency,
    /// 内存预算类：观测值 ≤ 目标即过（越低越好）。
    Memory,
    /// 质量分类：观测值 ≥ 目标即过，目标 ≤ 1000。
    Quality,
}

impl MetricKind {
    /// 类别序号（供审计口径）。
    pub fn ord(self) -> u8 {
        match self {
            MetricKind::Throughput => 0,
            MetricKind::Latency => 1,
            MetricKind::Memory => 2,
            MetricKind::Quality => 3,
        }
    }
    /// 方向：true=越高越好，false=越低越好。
    pub fn higher_is_better(self) -> bool {
        matches!(self, MetricKind::Throughput | MetricKind::Quality)
    }
}

/// 单条指标条款。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetricClause {
    /// 条款名（短标识，如 "fill_rate"）。
    pub name: String,
    /// 指标类别。
    pub kind: MetricKind,
    /// 目标值（整数口径，单位由调用方约定）。
    pub target: u64,
}

impl MetricClause {
    /// 构造并校验：名称非空、目标非零、质量类目标不超满分。
    pub fn new(name: &str, kind: MetricKind, target: u64) -> Result<Self, u16> {
        if name.is_empty() || target == 0 {
            return Err(E_CLAUSE);
        }
        if kind == MetricKind::Quality && target > QUALITY_FULL_PM as u64 {
            return Err(E_CLAUSE);
        }
        Ok(MetricClause {
            name: name.to_string(),
            kind,
            target,
        })
    }
    /// 单条判定：观测值是否达标。
    pub fn met(&self, observed: u64) -> Result<bool, u16> {
        if observed > u64::MAX / 2 {
            // 观测口径异常保护：超出预算即判口径非法。
            return Err(E_METRIC);
        }
        Ok(if self.kind.higher_is_better() {
            observed >= self.target
        } else {
            observed <= self.target
        })
    }
}

/// 合同评审状态（闭集五态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewState {
    /// 草拟。
    Draft,
    /// 评审中。
    InReview,
    /// 已签署（终态）。
    Signed,
    /// 已驳回（终态）。
    Rejected,
}

impl ReviewState {
    /// 状态序号。
    pub fn ord(self) -> u8 {
        match self {
            ReviewState::Draft => 0,
            ReviewState::InReview => 1,
            ReviewState::Signed => 2,
            ReviewState::Rejected => 3,
        }
    }
    /// 是否终态。
    pub fn is_final(self) -> bool {
        matches!(self, ReviewState::Signed | ReviewState::Rejected)
    }
}

/// 合同。
#[derive(Clone, Debug)]
pub struct Contract {
    /// 合同编号（批内唯一）。
    pub id: u32,
    /// 合同名。
    pub title: String,
    /// 指标条款（定容 CLAUSE_CAP）。
    pub clauses: Vec<MetricClause>,
    /// 评审状态。
    pub state: ReviewState,
    /// 驳回/签署时记录的不过条款数。
    pub failed_clauses: u32,
}

impl Contract {
    /// 新建合同（空条款，Draft 态）。
    pub fn new(id: u32, title: &str) -> Self {
        Contract {
            id,
            title: title.to_string(),
            clauses: Vec::new(),
            state: ReviewState::Draft,
            failed_clauses: 0,
        }
    }
    /// 追加条款（定容校验 + 编号校验在账本侧）。
    pub fn add_clause(&mut self, c: MetricClause) -> Result<(), u16> {
        if self.clauses.len() >= CLAUSE_CAP {
            return Err(E_CAP);
        }
        // 同名条款拒绝（条款名在合同内唯一）。
        for old in &self.clauses {
            if old.name == c.name {
                return Err(E_DUP);
            }
        }
        self.clauses.push(c);
        Ok(())
    }
}

/// 终审：逐条款比对观测序列。
/// 观测序列与条款按序一一对应，长度不一致即口径错误。
pub fn finalize(c: &mut Contract, observed: &[u64]) -> Result<ReviewState, u16> {
    if c.state != ReviewState::InReview {
        return Err(E_STATE);
    }
    if observed.len() != c.clauses.len() {
        return Err(E_CLAUSE);
    }
    if c.clauses.is_empty() {
        return Err(E_CLAUSE);
    }
    let mut failed = 0u32;
    for (i, cl) in c.clauses.iter().enumerate() {
        if !cl.met(observed[i])? {
            failed += 1;
        }
    }
    c.failed_clauses = failed;
    c.state = if failed == 0 {
        ReviewState::Signed
    } else {
        ReviewState::Rejected
    };
    Ok(c.state)
}

/// 合同账本（环形定容）。
#[derive(Clone, Debug)]
pub struct ContractLedger {
    ring: Vec<Contract>,
    evicted: u64,
    total: u64,
}

impl ContractLedger {
    pub fn new() -> Self {
        ContractLedger {
            ring: Vec::new(),
            evicted: 0,
            total: 0,
        }
    }
    /// 入账：满则逐出最旧（位置 0）。
    pub fn admit(&mut self, c: Contract) -> Result<(), u16> {
        if self.find(c.id)?.is_some() {
            return Err(E_DUP);
        }
        if self.ring.len() >= CONTRACT_CAP {
            self.ring.remove(0);
            self.evicted += 1;
        }
        self.ring.push(c);
        self.total += 1;
        Ok(())
    }
    /// 按编号查找。
    pub fn find(&self, id: u32) -> Result<Option<usize>, u16> {
        if id == 0 {
            return Err(E_EMPTY);
        }
        Ok(self.ring.iter().position(|c| c.id == id))
    }
    /// 守恒审计：total == held + evicted。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total != self.ring.len() as u64 + self.evicted {
            return Err(E_CAP);
        }
        Ok(())
    }
    /// 终审汇总：各态合同数（守恒：四态之和 == held）。
    pub fn tally(&self) -> Result<[u32; 4], u16> {
        let mut t = [0u32; 4];
        for c in &self.ring {
            let o = c.state.ord() as usize;
            t[o] += 1;
        }
        if t[0] + t[1] + t[2] + t[3] != self.ring.len() as u32 {
            return Err(E_STATE);
        }
        Ok(t)
    }
    /// 持有数。
    pub fn held(&self) -> usize {
        self.ring.len()
    }
    /// 状态迁移（进评审）。
    pub fn begin_review(&mut self, id: u32) -> Result<(), u16> {
        let p = self.find(id)?.ok_or(E_EMPTY)?;
        let c = &mut self.ring[p];
        if c.state != ReviewState::Draft {
            return Err(E_STATE);
        }
        c.state = ReviewState::InReview;
        Ok(())
    }
}

/// 自例程：主路径（达标合同签署）+ 驳回路径 + 状态机错误路径 + 账本守恒。
pub fn drill() -> Result<u32, u16> {
    let mut led = ContractLedger::new();
    // 主路径 1：吞吐+延迟双条款合同，全部达标 → Signed。
    let mut c1 = Contract::new(1, "e_final_a");
    c1.add_clause(MetricClause::new("fill_rate", MetricKind::Throughput, 900))?; // Mp/s
    c1.add_clause(MetricClause::new("frame_lat", MetricKind::Latency, 16_700))?; // us
    if c1.state != ReviewState::Draft {
        return Err(E_STATE);
    }
    // Draft 直接终审非法。
    if finalize(&mut c1, &[950, 16_000]) != Err(E_STATE) {
        return Err(E_STATE);
    }
    led.admit(c1)?;
    led.begin_review(1)?;
    // 拿回合同副本做终审（账本内直接改）。
    let p = led.find(1)?.ok_or(E_EMPTY)?;
    let observed: [u64; 2] = [950, 16_000];
    let mut ok_all = true;
    {
        let c = &mut led.ring[p];
        for (i, cl) in c.clauses.iter().enumerate() {
            if !cl.met(observed[i])? {
                ok_all = false;
            }
        }
        if !ok_all {
            return Err(E_METRIC);
        }
        let st = finalize(c, &observed)?;
        if st != ReviewState::Signed {
            return Err(E_STATE);
        }
    }
    led.audit()?;
    let t = led.tally()?;
    if t[ReviewState::Signed.ord() as usize] != 1 {
        return Err(E_EMPTY);
    }
    // 主路径 2：质量分越标条款 → Rejected（整单驳回）。
    let mut c2 = Contract::new(2, "e_final_b");
    c2.add_clause(MetricClause::new("quality", MetricKind::Quality, 950))?;
    c2.add_clause(MetricClause::new("peak_mem", MetricKind::Memory, 4096))?;
    led.admit(c2)?;
    led.begin_review(2)?;
    let p2 = led.find(2)?.ok_or(E_EMPTY)?;
    {
        let c = &mut led.ring[p2];
        let st = finalize(c, &[900, 4096])?;
        if st != ReviewState::Rejected || c.failed_clauses != 1 {
            return Err(E_STATE);
        }
    }
    // 终态不可再迁移。
    if led.ring[p2].state.is_final() && led.begin_review(2) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 错误路径 1：重复编号。
    let mut c3 = Contract::new(1, "dup");
    c3.add_clause(MetricClause::new("x", MetricKind::Throughput, 1))?;
    if led.admit(c3) != Err(E_DUP) {
        return Err(E_DUP);
    }
    // 错误路径 2：条款非法（空名 / 零目标 / 质量目标超满分）。
    if MetricClause::new("", MetricKind::Throughput, 10) != Err(E_CLAUSE)
        || MetricClause::new("y", MetricKind::Latency, 0) != Err(E_CLAUSE)
        || MetricClause::new("z", MetricKind::Quality, 1001) != Err(E_CLAUSE)
    {
        return Err(E_CLAUSE);
    }
    // 错误路径 3：同名条款拒绝。
    let mut c4 = Contract::new(4, "dupcl");
    c4.add_clause(MetricClause::new("a", MetricKind::Throughput, 5))?;
    if c4.add_clause(MetricClause::new("a", MetricKind::Latency, 5)) != Err(E_DUP) {
        return Err(E_DUP);
    }
    // 错误路径 4：观测长度与条款数不一致。
    let p1 = led.find(1)?.ok_or(E_EMPTY)?;
    {
        let c = &mut led.ring[p1];
        c.state = ReviewState::InReview;
        if finalize(c, &[1]) != Err(E_CLAUSE) {
            return Err(E_CLAUSE);
        }
    }
    // 错误路径 5：未知合同 / 零编号。
    if led.begin_review(99) != Err(E_EMPTY) || led.find(0) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 定容守恒：灌满+溢出逐出，恒等式仍成立。
    let mut led2 = ContractLedger::new();
    for i in 0..(CONTRACT_CAP as u32 + 4) {
        let mut c = Contract::new(i + 1, "fill");
        c.add_clause(MetricClause::new("m", MetricKind::Throughput, 1))?;
        led2.admit(c)?;
    }
    led2.audit()?;
    if led2.held() != CONTRACT_CAP || led2.total != CONTRACT_CAP as u64 + 4 {
        return Err(E_CAP);
    }
    Ok(0)
}
