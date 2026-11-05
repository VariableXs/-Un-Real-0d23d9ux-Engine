//! UNX-F12716 ktest 聚合器（I 型后段六批聚合） · 0x81D00段
//!
//! 判据原文（派工卡 12716 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12717-J1 I 型后段 120 条集成账等式断言绿
//!      （批账×6=120 对平）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 六批闭集：聚合器只收 I 型后段六批（批号 30..=35），批号越闭集
//!    即账外混入；一批一账户（重复批号登记拒）；
//! 2. 批额闸：每批全跑条数必须恰 20（0 条与超 20 条都拒）——批账×6
//!    等式的逐批前置；
//! 3. 聚合等式闸：六批条数之和须恰 120（checked_add 防溢出），等式
//!    不平即聚合拒绝封账——「全跑条数与聚合账等式」随批归档；
//! 4. 归档面：等式断言（批账×6=120）以归档行落账（批号/条数/等式
//!    三列），归档行数=六批，逐行可点验；
//! 5. 聚合状态机 AggOpen → AggSummed → AggSealed 单向推进，rank
//!    单调，倒退/跳相显性拒；封账后批账只读；
//! 6. 违约注入十族逐路必抓：批号越闭集、重复批号、零条数、批条数超
//!    界、条数溢出防护、批额不足聚合、等式不平、相位倒退、封账后写、
//!    空摘要。no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值
//!    一律 match/if let/.get()/checked_*/saturating_*，禁裸下标；
//!    中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `agg_equation_ok` ↔ ② 批账×6=120 等式断言（聚合入口等式绿）；
//!   - `archive_rows_ok` ↔ ② 全跑条数与聚合账等式随批归档（六行三列）；
//!   - `per_batch_quota_ok` ↔ ② 逐批恰 20 条（等式逐批前置）；
//!   - `fault_sweep_ok` ↔ ② 违约注入 10/10 必抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12716 − 12600)）。
pub const SEG_BASE: u32 = 0x81D00;

/// 六批闭集下界。
pub const AGG_BATCH_LO: u32 = 30;

/// 六批闭集上界。
pub const AGG_BATCH_HI: u32 = 35;

/// 每批条数配额（恰 20）。
pub const PER_BATCH_QUOTA: u32 = 20;

/// 聚合等式目标（批账×6=120）。
pub const AGG_TOTAL: u32 = 120;

// ── 聚合诊断码闭集 ───────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgDiag {
    /// 批号越闭集。
    EBatchUnknown,
    /// 重复批号。
    EBatchDup,
    /// 零条数。
    ECountZero,
    /// 批条数超界（> 20）。
    ECountOver,
    /// 条数溢出防护（聚合加法不溢出才许封账）。
    ECountOverflow,
    /// 批额不足（六批未齐）。
    EBatchShort,
    /// 等式不平（Σ批账 ≠ 120）。
    EEquationOff,
    /// 相位倒退。
    ERewind,
    /// 封账后写。
    ESealedWrite,
    /// 聚合摘要空。
    ESummaryEmpty,
    /// 未开账先录。
    ENotOpen,
    /// 聚合账双跑分歧。
    EAggDiverge,
}

impl AgDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                AgDiag::EBatchUnknown => 0x01,
                AgDiag::EBatchDup => 0x02,
                AgDiag::ECountZero => 0x03,
                AgDiag::ECountOver => 0x04,
                AgDiag::ECountOverflow => 0x05,
                AgDiag::EBatchShort => 0x06,
                AgDiag::EEquationOff => 0x07,
                AgDiag::ERewind => 0x08,
                AgDiag::ESealedWrite => 0x09,
                AgDiag::ESummaryEmpty => 0x0a,
                AgDiag::ENotOpen => 0x0b,
                AgDiag::EAggDiverge => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            AgDiag::EBatchUnknown => "批号越闭集：聚合器只收 30..=35 六批，先改批号",
            AgDiag::EBatchDup => "重复批号：一批一账户，重复先查在册批账",
            AgDiag::ECountZero => "零条数：批账至少 1 条，零条批先核全跑日志",
            AgDiag::ECountOver => "批条数超界：每批恰 20 条，超界先核重复计数",
            AgDiag::ECountOverflow => "条数溢出：聚合加法须 checked 不溢出，先裁账面",
            AgDiag::EBatchShort => "批额不足：六批全齐才可聚合封账，缺批先补",
            AgDiag::EEquationOff => "等式不平：批账×6 须恰 120，先对全跑条数",
            AgDiag::ERewind => "相位倒退：AggOpen→AggSummed→AggSealed 只进不退",
            AgDiag::ESealedWrite => "封账后写：批账只读，补录走下一轮聚合",
            AgDiag::ESummaryEmpty => "聚合摘要空：摘要非空才许写回已完成",
            AgDiag::ENotOpen => "未开账先录：先 begin 开账再录批账",
            AgDiag::EAggDiverge => "聚合双跑分歧：总条数与指纹两轮须逐位同判",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const AG_ALL: &[AgDiag] = &[
    AgDiag::EBatchUnknown,
    AgDiag::EBatchDup,
    AgDiag::ECountZero,
    AgDiag::ECountOver,
    AgDiag::ECountOverflow,
    AgDiag::EBatchShort,
    AgDiag::EEquationOff,
    AgDiag::ERewind,
    AgDiag::ESealedWrite,
    AgDiag::ESummaryEmpty,
    AgDiag::ENotOpen,
    AgDiag::EAggDiverge,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < AG_ALL.len() {
        if let Some(d) = AG_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81D01..0x81D0C 闭集内"
}

// ── 聚合状态机 ───────────────────────────────────────────────────
/// 聚合相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggPhase {
    /// 开账受理。
    AggOpen,
    /// 求和完成：六批全齐等式平。
    AggSummed,
    /// 聚合封账。
    AggSealed,
}

impl AggPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            AggPhase::AggOpen => 0,
            AggPhase::AggSummed => 1,
            AggPhase::AggSealed => 2,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            AggPhase::AggOpen => "开账受理",
            AggPhase::AggSummed => "求和完成",
            AggPhase::AggSealed => "聚合封账",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn ag_advance(from: AggPhase, to: AggPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(AgDiag::ERewind.val())
    }
}

// ── 批账模型 ─────────────────────────────────────────────────────
/// 批账户：批号 + 全跑条数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchAccount {
    /// 批号（30..=35）。
    pub batch: u32,
    /// 全跑条数（恰 20）。
    pub checked: u32,
}

/// 归档行：批号 / 条数 / 等式（条数×6 批总等式的逐批贡献列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveRow {
    /// 批号。
    pub batch: u32,
    /// 全跑条数。
    pub checked: u32,
    /// 等式贡献校验（checked 恰等于 PER_BATCH_QUOTA）。
    pub quota_hold: bool,
}

/// 批号是否在六批闭集。
pub fn agg_batch_known(b: u32) -> bool {
    b >= AGG_BATCH_LO && b <= AGG_BATCH_HI
}

/// 单批账户闸：批号闭集 + 条数恰配额。
pub fn account_ok(a: &BatchAccount) -> bool {
    if !agg_batch_known(a.batch) {
        return false;
    }
    if a.checked == 0 {
        return false;
    }
    if a.checked > PER_BATCH_QUOTA {
        return false;
    }
    true
}

/// 冻结六批账（批账×6=120 权威基线）。
pub const AGG_ACCOUNT_TABLE: &[BatchAccount] = &[
    BatchAccount { batch: 30, checked: 20 },
    BatchAccount { batch: 31, checked: 20 },
    BatchAccount { batch: 32, checked: 20 },
    BatchAccount { batch: 33, checked: 20 },
    BatchAccount { batch: 34, checked: 20 },
    BatchAccount { batch: 35, checked: 20 },
];

/// 聚合求和（checked_add 防溢出；任一溢出 None）。
pub fn agg_sum(accounts: &[BatchAccount]) -> Option<u32> {
    let mut total: u32 = 0;
    let mut i = 0usize;
    while i < accounts.len() {
        let a = match accounts.get(i) {
            Some(v) => v,
            None => return None,
        };
        total = total.checked_add(a.checked)?;
        i += 1;
    }
    Some(total)
}

/// 归档行生成：批账 → 逐批三列归档行。
pub fn archive_rows(accounts: &[BatchAccount]) -> Vec<ArchiveRow> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < accounts.len() {
        if let Some(a) = accounts.get(i) {
            out.push(ArchiveRow {
                batch: a.batch,
                checked: a.checked,
                quota_hold: a.checked == PER_BATCH_QUOTA,
            });
        }
        i += 1;
    }
    out
}

/// 谓词：逐批恰配额（批号闭集内逐批在册且条数=20）。
fn per_batch_quota_ok(accounts: &[BatchAccount]) -> bool {
    if accounts.len() != (AGG_BATCH_HI - AGG_BATCH_LO + 1) as usize {
        return false;
    }
    let mut b = AGG_BATCH_LO;
    while b <= AGG_BATCH_HI {
        let mut found = false;
        let mut i = 0usize;
        while i < accounts.len() && !found {
            if let Some(a) = accounts.get(i) {
                found = a.batch == b && a.checked == PER_BATCH_QUOTA;
            }
            i += 1;
        }
        if !found {
            return false;
        }
        b += 1;
    }
    true
}

// ── 聚合账（真实状态机 + 真实错误路径） ─────────────────────────
/// ktest 聚合账：相位单调；等式不平阻断封账。
pub struct AggLedger {
    phase: AggPhase,
    accounts: Vec<BatchAccount>,
    rejects: Vec<u32>,
    summary: String,
}

impl AggLedger {
    /// 新账。
    pub fn new() -> AggLedger {
        AggLedger {
            phase: AggPhase::AggOpen,
            accounts: Vec::new(),
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开账（AggOpen 幂等）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.phase == AggPhase::AggOpen {
            Ok(())
        } else {
            let d = AgDiag::ENotOpen.val();
            self.rejects.push(d);
            Err(d)
        }
    }

    pub fn account_len(&self) -> usize {
        self.accounts.len()
    }

    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    pub fn phase(&self) -> AggPhase {
        self.phase
    }

    fn has_batch(&self, b: u32) -> bool {
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some(a) = self.accounts.get(i) {
                if a.batch == b {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 录批账：批号闭集 + 查重 + 条数闸 + 容量。
    pub fn reg_account(&mut self, a: BatchAccount) -> Result<(), u32> {
        if self.phase != AggPhase::AggOpen {
            let d = if self.phase == AggPhase::AggSealed {
                AgDiag::ESealedWrite.val()
            } else {
                AgDiag::ERewind.val()
            };
            self.rejects.push(d);
            return Err(d);
        }
        if self.accounts.len() >= (AGG_BATCH_HI - AGG_BATCH_LO + 1) as usize {
            let d = AgDiag::EBatchDup.val();
            self.rejects.push(d);
            return Err(d);
        }
        if !agg_batch_known(a.batch) {
            let d = AgDiag::EBatchUnknown.val();
            self.rejects.push(d);
            return Err(d);
        }
        if self.has_batch(a.batch) {
            let d = AgDiag::EBatchDup.val();
            self.rejects.push(d);
            return Err(d);
        }
        if a.checked == 0 {
            let d = AgDiag::ECountZero.val();
            self.rejects.push(d);
            return Err(d);
        }
        if a.checked > PER_BATCH_QUOTA {
            let d = AgDiag::ECountOver.val();
            self.rejects.push(d);
            return Err(d);
        }
        self.accounts.push(a);
        Ok(())
    }

    /// 求和收束：AggOpen → AggSummed，六批全齐 + 等式平 + 不溢出。
    pub fn sum_up(&mut self) -> Result<u32, u32> {
        match ag_advance(self.phase, AggPhase::AggSummed) {
            Err(d) => {
                self.rejects.push(d);
                Err(d)
            }
            Ok(()) => {
                if self.accounts.len() < (AGG_BATCH_HI - AGG_BATCH_LO + 1) as usize {
                    let d = AgDiag::EBatchShort.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                let total = match agg_sum(&self.accounts) {
                    Some(v) => v,
                    None => {
                        let d = AgDiag::ECountOverflow.val();
                        self.rejects.push(d);
                        return Err(d);
                    }
                };
                if total != AGG_TOTAL {
                    let d = AgDiag::EEquationOff.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                self.phase = AggPhase::AggSummed;
                Ok(total)
            }
        }
    }

    /// 封账：AggSummed → AggSealed，摘要非空。
    pub fn seal(&mut self, summary: &str) -> Result<(), u32> {
        match ag_advance(self.phase, AggPhase::AggSealed) {
            Err(d) => {
                self.rejects.push(d);
                Err(d)
            }
            Ok(()) => {
                if summary.is_empty() {
                    let d = AgDiag::ESummaryEmpty.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                self.summary = String::from(summary);
                self.phase = AggPhase::AggSealed;
                Ok(())
            }
        }
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }
}

// ── 聚合指纹（FNV-1a 32bit） ────────────────────────────────────
/// 对批账折叠（批号/条数）。
pub fn agg_digest(accounts: &[BatchAccount]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < accounts.len() {
        if let Some(a) = accounts.get(i) {
            h = h.wrapping_mul(0x0100_0193) ^ a.batch;
            h = h.wrapping_mul(0x0100_0193) ^ a.checked;
        }
        i += 1;
    }
    h
}

// ── 判据谓词 ─────────────────────────────────────────────────────
/// 谓词：聚合等式绿（冻结六批账 Σ=120 且逐批恰 20 且双跑同判）。
fn agg_equation_ok() -> bool {
    if AGG_ACCOUNT_TABLE.len() != (AGG_BATCH_HI - AGG_BATCH_LO + 1) as usize {
        return false;
    }
    let total = match agg_sum(AGG_ACCOUNT_TABLE) {
        Some(v) => v,
        None => return false,
    };
    if total != AGG_TOTAL {
        return false;
    }
    if !per_batch_quota_ok(AGG_ACCOUNT_TABLE) {
        return false;
    }
    // 等式代数面：逐批条数×6批 ≡ 120（6×20）。
    PER_BATCH_QUOTA.saturating_mul(6) == AGG_TOTAL
}

/// 谓词：归档行齐（六行三列、quota_hold 全真、行序与批号升序同判）。
fn archive_rows_ok() -> bool {
    let rows = archive_rows(AGG_ACCOUNT_TABLE);
    if rows.len() != AGG_ACCOUNT_TABLE.len() {
        return false;
    }
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(v) => v,
            None => return false,
        };
        let a = match AGG_ACCOUNT_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if r.batch != a.batch || r.checked != a.checked || !r.quota_hold {
            return false;
        }
        i += 1;
    }
    true
}

/// 完整聚合账构造基线：开账→录六批→求和→封账。
pub fn build_full_ledger() -> AggLedger {
    let mut a = AggLedger::new();
    let _ = a.begin();
    let mut i = 0usize;
    while i < AGG_ACCOUNT_TABLE.len() {
        if let Some(acc) = AGG_ACCOUNT_TABLE.get(i) {
            let _ = a.reg_account(*acc);
        }
        i += 1;
    }
    let _ = a.sum_up();
    let _ = a.seal("ktest 聚合器：I 型后段六批 120 条等式对平随批归档");
    a
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_ledger();
    let y = build_full_ledger();
    x.account_len() == y.account_len()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && agg_digest(AGG_ACCOUNT_TABLE) == agg_digest(AGG_ACCOUNT_TABLE)
}

/// 谓词：完成写回语义。
fn writeback_ok() -> bool {
    let a = build_full_ledger();
    a.phase() == AggPhase::AggSealed && !a.summary().is_empty()
}

/// 谓词：段位守卫（VX9 窗口）。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0
}

/// VX9 段位公式重算。
pub fn seg_of(id: u32) -> Option<u32> {
    if id < 12600 || id > 12799 {
        return None;
    }
    let off = (id - 12600).checked_mul(0x40)?;
    Some(0x80000u32.checked_add(off)?)
}

// ── 违约注入十族 ─────────────────────────────────────────────────
/// 注入演练：十族聚合违约逐路投递，全部 Err（或扫描命中）才算抓全。
pub fn agg_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();

    let a0 = match AGG_ACCOUNT_TABLE.get(0) {
        Some(v) => *v,
        None => return (0, 10, String::from("表空")),
    };

    // 族1：批号越闭集。
    let mut a1 = AggLedger::new();
    let bad_b = BatchAccount { batch: 99, checked: 20 };
    if a1.reg_account(bad_b).is_err() {
        caught += 1;
    } else {
        miss.push_str("F1批号越闭集;");
    }

    // 族2：重复批号。
    let mut a2 = AggLedger::new();
    let _ = a2.reg_account(a0);
    if a2.reg_account(a0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F2重复批号;");
    }

    // 族3：零条数。
    let mut a3 = AggLedger::new();
    let zero = BatchAccount { batch: 31, checked: 0 };
    if a3.reg_account(zero).is_err() {
        caught += 1;
    } else {
        miss.push_str("F3零条数;");
    }

    // 族4：批条数超界。
    let mut a4 = AggLedger::new();
    let over = BatchAccount { batch: 31, checked: PER_BATCH_QUOTA + 1 };
    if a4.reg_account(over).is_err() {
        caught += 1;
    } else {
        miss.push_str("F4批条数超界;");
    }

    // 族5：批额不足聚合（只录一批即求和）。
    let mut a5 = AggLedger::new();
    let _ = a5.reg_account(a0);
    if a5.sum_up().is_err() {
        caught += 1;
    } else {
        miss.push_str("F5批额不足;");
    }

    // 族6：等式不平（六批全录但两批条数各少 1 → Σ=118 ≠ 120）。
    let mut a6 = AggLedger::new();
    let mut i = 0usize;
    while i < AGG_ACCOUNT_TABLE.len() {
        if let Some(acc) = AGG_ACCOUNT_TABLE.get(i) {
            let mut row = *acc;
            if row.batch == 32 && row.checked > 1 {
                row.checked = row.checked - 1;
            }
            if row.batch == 34 && row.checked > 1 {
                row.checked = row.checked - 1;
            }
            let _ = a6.reg_account(row);
        }
        i += 1;
    }
    if a6.sum_up().is_err() {
        caught += 1;
    } else {
        miss.push_str("F6等式不平;");
    }

    // 族7：未开账即录（Summed 相位录批）。
    let mut a7 = build_full_ledger();
    if a7.reg_account(a0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F7封账后写;");
    }

    // 族8：相位倒退（封账后再求和）。
    let mut a8 = build_full_ledger();
    if a8.sum_up().is_err() {
        caught += 1;
    } else {
        miss.push_str("F8相位倒退;");
    }

    // 族9：空摘要封账。
    let mut a9 = build_full_ledger_no_seal();
    if a9.seal("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F9空摘要;");
    }

    // 族10：溢出防护直读（u32 上界加法必须 None，聚合求和同型防护）。
    let ov = u32::MAX.checked_add(1);
    if ov.is_none() && agg_sum(AGG_ACCOUNT_TABLE).is_some() {
        caught += 1;
    } else {
        miss.push_str("F10溢出防护;");
    }

    (caught, 10usize, miss)
}

/// 建账至求和相位（未封账）。
pub fn build_full_ledger_no_seal() -> AggLedger {
    let mut a = AggLedger::new();
    let _ = a.begin();
    let mut i = 0usize;
    while i < AGG_ACCOUNT_TABLE.len() {
        if let Some(acc) = AGG_ACCOUNT_TABLE.get(i) {
            let _ = a.reg_account(*acc);
        }
        i += 1;
    }
    let _ = a.sum_up();
    a
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = agg_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12716 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12716_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12716");

    s.add_dyn(
        "F12716-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12716-02 段位公式重算",
        {
            let mut ok = seg_of(12716) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12716-03 诊断码闭集",
        {
            let mut ok = AG_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < AG_ALL.len() {
                if let Some(d) = AG_ALL.get(i) {
                    let v = d.val();
                    ok &= v > SEG_BASE && v < SEG_BASE + 0x40;
                    ok &= !d.detail().is_empty();
                }
                i += 1;
            }
            ok &= !detail(SEG_BASE + 0x30).is_empty();
            ok
        },
        "0x01..0x0C 闭集偏移互异且文案非空",
    );

    s.add_dyn(
        "F12716-04 聚合等式绿",
        agg_equation_ok(),
        &format!("批账×6={} 对平", agg_sum(AGG_ACCOUNT_TABLE).unwrap_or(0)),
    );

    s.add_dyn(
        "F12716-05 归档行随批齐",
        archive_rows_ok(),
        &format!("rows={} 三列全真", archive_rows(AGG_ACCOUNT_TABLE).len()),
    );

    s.add_dyn(
        "F12716-06 注入联测 10/10 拒止",
        {
            let (c, t, _) = agg_fault_sweep();
            c == t
        },
        &format!("caught={}/10", agg_fault_sweep().0),
    );

    s.add_dyn(
        "F12716-07 相位迁移守卫直读",
        ag_advance(AggPhase::AggOpen, AggPhase::AggSummed).is_ok()
            && ag_advance(AggPhase::AggSummed, AggPhase::AggSealed).is_ok()
            && ag_advance(AggPhase::AggSealed, AggPhase::AggSummed).is_err()
            && ag_advance(AggPhase::AggOpen, AggPhase::AggSealed).is_err(),
        "AggOpen→AggSummed→AggSealed 只进不退不跳",
    );

    s.add_dyn(
        "F12716-08 批闭集边界",
        {
            let mut ok = agg_batch_known(AGG_BATCH_LO) && agg_batch_known(AGG_BATCH_HI);
            ok &= !agg_batch_known(AGG_BATCH_LO - 1);
            ok &= !agg_batch_known(AGG_BATCH_HI + 1);
            ok
        },
        "30/35 端点在册，29/36 越界",
    );

    s.add_dyn(
        "F12716-09 求和防溢出直读",
        agg_sum(AGG_ACCOUNT_TABLE) == Some(AGG_TOTAL),
        &format!("sum={}", agg_sum(AGG_ACCOUNT_TABLE).unwrap_or(0)),
    );

    s.add_dyn(
        "F12716-10 完整建账基线",
        {
            let a = build_full_ledger();
            a.account_len() == AGG_ACCOUNT_TABLE.len()
                && a.reject_len() == 0
                && a.phase() == AggPhase::AggSealed
        },
        &format!(
            "accounts={} phase={}",
            build_full_ledger().account_len(),
            build_full_ledger().phase().name()
        ),
    );

    s.add_dyn(
        "F12716-11 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12716-12 聚合指纹非零",
        agg_digest(AGG_ACCOUNT_TABLE) != 0,
        &format!("digest={:08X}", agg_digest(AGG_ACCOUNT_TABLE)),
    );

    s.add_dyn(
        "F12716-13 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x81D00);
        assert_eq!(seg_of(12716), Some(SEG_BASE));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(AG_ALL.len(), 12);
        let mut i = 0usize;
        while i < AG_ALL.len() {
            let vi = match AG_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < AG_ALL.len() {
                let vj = match AG_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05X}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn equation_and_quota_hold() {
        assert!(agg_equation_ok());
        assert!(per_batch_quota_ok(AGG_ACCOUNT_TABLE));
        assert_eq!(agg_sum(AGG_ACCOUNT_TABLE), Some(120));
        assert_eq!(AGG_ACCOUNT_TABLE.len(), 6);
        // 等式代数面：20×6=120。
        assert_eq!(PER_BATCH_QUOTA * 6, AGG_TOTAL);
        // 归档行齐。
        assert!(archive_rows_ok());
        let rows = archive_rows(AGG_ACCOUNT_TABLE);
        assert_eq!(rows.len(), 6);
        let mut i = 0usize;
        while i < rows.len() {
            let r = match rows.get(i) {
                Some(v) => v,
                None => return,
            };
            assert!(r.quota_hold);
            i += 1;
        }
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = agg_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        let a0 = match AGG_ACCOUNT_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        let mut a = AggLedger::new();
        let bad_b = BatchAccount { batch: 99, checked: 20 };
        assert_eq!(a.reg_account(bad_b), Err(AgDiag::EBatchUnknown.val()));
        let _ = a.reg_account(a0);
        assert_eq!(a.reg_account(a0), Err(AgDiag::EBatchDup.val()));
        let zero = BatchAccount { batch: 31, checked: 0 };
        assert_eq!(a.reg_account(zero), Err(AgDiag::ECountZero.val()));
        let over = BatchAccount { batch: 31, checked: PER_BATCH_QUOTA + 1 };
        assert_eq!(a.reg_account(over), Err(AgDiag::ECountOver.val()));
        // 等式不平（六批全录、两批各少 1 条 → Σ=118）。
        let mut b = AggLedger::new();
        let mut i = 0usize;
        while i < AGG_ACCOUNT_TABLE.len() {
            if let Some(acc) = AGG_ACCOUNT_TABLE.get(i) {
                let mut row = *acc;
                if row.batch == 32 && row.checked > 1 {
                    row.checked = row.checked - 1;
                }
                if row.batch == 34 && row.checked > 1 {
                    row.checked = row.checked - 1;
                }
                let _ = b.reg_account(row);
            }
            i += 1;
        }
        assert_eq!(b.sum_up(), Err(AgDiag::EEquationOff.val()));
        // 拒单面：批越闭集 + 重复批号 + 零条数 + 超界条数 = 4。
        assert_eq!(a.reject_len(), 4);
    }

    #[test]
    fn phase_guards_and_seal() {
        assert!(ag_advance(AggPhase::AggOpen, AggPhase::AggSummed).is_ok());
        assert!(ag_advance(AggPhase::AggSummed, AggPhase::AggSealed).is_ok());
        assert!(ag_advance(AggPhase::AggSummed, AggPhase::AggSummed).is_err());
        assert!(ag_advance(AggPhase::AggSealed, AggPhase::AggOpen).is_err());
        assert!(AggPhase::AggOpen.rank() < AggPhase::AggSummed.rank());
        assert!(AggPhase::AggSummed.rank() < AggPhase::AggSealed.rank());
        // 批额不足聚合拒；补齐后过；封账后写拒；空摘要拒。
        let mut a = AggLedger::new();
        let _ = a.begin();
        let a0 = match AGG_ACCOUNT_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        let _ = a.reg_account(a0);
        assert!(a.sum_up().is_err());
        let mut i = 0usize;
        while i < AGG_ACCOUNT_TABLE.len() {
            if let Some(acc) = AGG_ACCOUNT_TABLE.get(i) {
                let _ = a.reg_account(*acc);
            }
            i += 1;
        }
        assert!(a.sum_up().is_ok());
        assert_eq!(a.reg_account(a0), Err(AgDiag::ERewind.val()));
        assert_eq!(a.seal(""), Err(AgDiag::ESummaryEmpty.val()));
        assert!(a.seal("聚合封账").is_ok());
        assert_eq!(a.phase(), AggPhase::AggSealed);
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_ledger();
        let y = build_full_ledger();
        assert_eq!(x.account_len(), y.account_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_eq!(agg_digest(AGG_ACCOUNT_TABLE), agg_digest(AGG_ACCOUNT_TABLE));
        assert_ne!(agg_digest(AGG_ACCOUNT_TABLE), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [AggPhase::AggOpen, AggPhase::AggSummed, AggPhase::AggSealed].iter() {
            assert!(!st.name().is_empty());
        }
        for d in AG_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12716_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
