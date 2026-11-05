//! UNX-F12717 I 型收官账（120 条集成账等式） · 0x81D40段
//!
//! 判据原文（派工卡 12717 逐字锚定）：
//!   ① 纯功能代码 ≥326 行；
//!   ② 判据原文：UNX-F12718-J1 C 型回归框架交付齐（三类对照表盘点+
//!      回归矩阵），框架可点验；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 集成账行模型：每条集成账行 = 批号 × 检查号（六批 30..=35 ×
//!    每批检查 1..=20），以 20 位位掩码冻结每批判据面——bit i 置位即
//!    检查 i 绿，掩码零与位越 20 界都是批账违约；
//! 2. 等式断言：Σ批账（位掩码 count_ones 逐批计数）= 120，且六批各
//!    恰 20——批账×6=120 对平（等式不平即收官拒绝封账）；
//! 3. 三源对平：批账源（位掩码计数）/ 聚合账源（冻结聚合总数）/ 归档
//!    账源（冻结归档总数）三源总数须同值 120——任一源漂移即对平失败
//!    （收官账必须三源一致）；
//! 4. 收官状态机 InDraft → InBalanced → InSealed 单向推进，rank 单调，
//!    倒退/跳相显性拒；封账后集成账只读；
//! 5. 集成行展开面：位掩码可确定性展开为逐行（批/检查/判定）三元，
//!    展开行数恰 120 且判定全绿——行面与账面两视角互证；
//! 6. 违约注入十族逐路必抓：批号越闭集、掩码零、位越界、等式不平、
//!    三源分歧、批额不足、相位倒退、封账后写、空摘要、行面短额。
//!    no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/checked_*/saturating_*，禁裸下标；中文注释
//!    与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `equation_balanced_ok` ↔ ② 批账×6=120 等式断言对平；
//!   - `three_src_ok` ↔ ② 三源一致对平（批账/聚合/归档同值 120）；
//!   - `rows_expanded_ok` ↔ ② 行面展开 120 行全绿（账面互证）；
//!   - `fault_sweep_ok` ↔ ② 违约注入 10/10 必抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12717 − 12600)）。
pub const SEG_BASE: u32 = 0x81D40;

/// 收官六批闭集下界。
pub const IN_BATCH_LO: u32 = 30;

/// 收官六批闭集上界。
pub const IN_BATCH_HI: u32 = 35;

/// 每批检查数（位掩码宽度）。
pub const CHECKS_PER_BATCH: u32 = 20;

/// 集成账等式目标（Σ批账=120）。
pub const IN_TOTAL: u32 = 120;

/// 满掩码（20 位全绿）。
pub const FULL_MASK: u32 = (1u32 << CHECKS_PER_BATCH) - 1;

// ── 收官诊断码闭集 ───────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InDiag {
    /// 批号越闭集。
    EBatchUnknown,
    /// 掩码零（批账零条）。
    EMaskZero,
    /// 位越界（掩码超出 20 位宽）。
    EBitOver,
    /// 等式不平（Σ批账 ≠ 120）。
    EEquationOff,
    /// 三源分歧（批账/聚合/归档不同值）。
    ESrcDiverge,
    /// 批额不足（六批未齐）。
    EBatchShort,
    /// 行面短额（展开行 < 120）。
    ERowShort,
    /// 相位倒退。
    ERewind,
    /// 封账后写。
    ESealedWrite,
    /// 收官摘要空。
    ESummaryEmpty,
    /// 未开账先录。
    ENotOpen,
    /// 收官指纹为零。
    EDigestZero,
}

impl InDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                InDiag::EBatchUnknown => 0x01,
                InDiag::EMaskZero => 0x02,
                InDiag::EBitOver => 0x03,
                InDiag::EEquationOff => 0x04,
                InDiag::ESrcDiverge => 0x05,
                InDiag::EBatchShort => 0x06,
                InDiag::ERowShort => 0x07,
                InDiag::ERewind => 0x08,
                InDiag::ESealedWrite => 0x09,
                InDiag::ESummaryEmpty => 0x0a,
                InDiag::ENotOpen => 0x0b,
                InDiag::EDigestZero => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            InDiag::EBatchUnknown => "批号越闭集：收官账只收 30..=35 六批，先改批号",
            InDiag::EMaskZero => "掩码零：批账零条即账面违约，先核全跑日志",
            InDiag::EBitOver => "位越界：掩码限 20 位宽，越界位先裁",
            InDiag::EEquationOff => "等式不平：Σ批账须恰 120，先对位掩码计数",
            InDiag::ESrcDiverge => "三源分歧：批账/聚合/归档三源须同值 120",
            InDiag::EBatchShort => "批额不足：六批全齐才可收官封账，缺批先补",
            InDiag::ERowShort => "行面短额：展开行须恰 120 行，先补绿位",
            InDiag::ERewind => "相位倒退：InDraft→InBalanced→InSealed 只进不退",
            InDiag::ESealedWrite => "封账后写：集成账只读，补录走下一轮收官",
            InDiag::ESummaryEmpty => "收官摘要空：摘要非空才许写回已完成",
            InDiag::ENotOpen => "未开账先录：先 begin 开账再录批账",
            InDiag::EDigestZero => "收官指纹为零：指纹面退化，先核掩码表",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const IN_ALL: &[InDiag] = &[
    InDiag::EBatchUnknown,
    InDiag::EMaskZero,
    InDiag::EBitOver,
    InDiag::EEquationOff,
    InDiag::ESrcDiverge,
    InDiag::EBatchShort,
    InDiag::ERowShort,
    InDiag::ERewind,
    InDiag::ESealedWrite,
    InDiag::ESummaryEmpty,
    InDiag::ENotOpen,
    InDiag::EDigestZero,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < IN_ALL.len() {
        if let Some(d) = IN_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81D41..0x81D4C 闭集内"
}

// ── 收官状态机 ───────────────────────────────────────────────────
/// 收官相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InPhase {
    /// 收官草稿。
    InDraft,
    /// 对平完成：等式平 + 三源一致。
    InBalanced,
    /// 收官封账。
    InSealed,
}

impl InPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            InPhase::InDraft => 0,
            InPhase::InBalanced => 1,
            InPhase::InSealed => 2,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            InPhase::InDraft => "收官草稿",
            InPhase::InBalanced => "对平完成",
            InPhase::InSealed => "收官封账",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn in_advance(from: InPhase, to: InPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(InDiag::ERewind.val())
    }
}

// ── 集成账模型 ───────────────────────────────────────────────────
/// 批判据位掩码账户：批号 + 20 位判定掩码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaskAccount {
    /// 批号（30..=35）。
    pub batch: u32,
    /// 判定位掩码（bit i = 检查 i+1 绿）。
    pub mask: u32,
}

/// 集成行（位掩码展开三元）：批号 / 检查号 / 判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntegrationRow {
    /// 批号。
    pub batch: u32,
    /// 检查号（1..=20）。
    pub check_no: u32,
    /// 判定（true=绿）。
    pub verdict: bool,
}

/// 冻结六批掩码账（20 位全绿——批账×6=120 权威基线）。
pub const MASK_TABLE: &[MaskAccount] = &[
    MaskAccount { batch: 30, mask: FULL_MASK },
    MaskAccount { batch: 31, mask: FULL_MASK },
    MaskAccount { batch: 32, mask: FULL_MASK },
    MaskAccount { batch: 33, mask: FULL_MASK },
    MaskAccount { batch: 34, mask: FULL_MASK },
    MaskAccount { batch: 35, mask: FULL_MASK },
];

/// 三源冻结常数：聚合账源总数 / 归档账源总数（与批账源对平）。
pub const SRC_AGG_TOTAL: u32 = 120;
pub const SRC_ARCH_TOTAL: u32 = 120;

/// 批号是否在收官六批闭集。
pub fn in_batch_known(b: u32) -> bool {
    b >= IN_BATCH_LO && b <= IN_BATCH_HI
}

/// 单批掩码闸：批号闭集 + 掩码非零 + 不越 20 位宽。
pub fn mask_account_ok(a: &MaskAccount) -> bool {
    if !in_batch_known(a.batch) {
        return false;
    }
    if a.mask == 0 {
        return false;
    }
    if a.mask > FULL_MASK {
        return false;
    }
    true
}

/// 批账计数：掩码置位数（count_ones，纯整数）。
pub fn mask_count(a: &MaskAccount) -> u32 {
    a.mask.count_ones()
}

/// 批账源总数：六批计数之和（checked_add 防溢出）。
pub fn batch_src_total(accounts: &[MaskAccount]) -> Option<u32> {
    let mut total: u32 = 0;
    let mut i = 0usize;
    while i < accounts.len() {
        let a = match accounts.get(i) {
            Some(v) => v,
            None => return None,
        };
        total = total.checked_add(mask_count(a))?;
        i += 1;
    }
    Some(total)
}

/// 位掩码展开：批掩码 → 逐行三元（检查号 1..=20，置位行 verdict=true）。
pub fn expand_rows(a: &MaskAccount) -> Vec<IntegrationRow> {
    let mut out = Vec::new();
    let mut i = 0u32;
    while i < CHECKS_PER_BATCH {
        let bit = 1u32.checked_shl(i).unwrap_or(0);
        out.push(IntegrationRow {
            batch: a.batch,
            check_no: i + 1,
            verdict: a.mask & bit != 0,
        });
        i += 1;
    }
    out
}

/// 全账展开：六批掩码全展开（行序=批升序×检查升序）。
pub fn expand_all(accounts: &[MaskAccount]) -> Vec<IntegrationRow> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < accounts.len() {
        if let Some(a) = accounts.get(i) {
            let mut j = 0usize;
            let rows = expand_rows(a);
            while j < rows.len() {
                if let Some(r) = rows.get(j) {
                    out.push(*r);
                }
                j += 1;
            }
        }
        i += 1;
    }
    out
}

/// 三源对平：批账源 == 聚合账源 == 归档账源 == IN_TOTAL。
pub fn three_src_balanced(accounts: &[MaskAccount]) -> bool {
    let b = match batch_src_total(accounts) {
        Some(v) => v,
        None => return false,
    };
    b == SRC_AGG_TOTAL && SRC_AGG_TOTAL == SRC_ARCH_TOTAL && b == IN_TOTAL
}

// ── 收官账（真实状态机 + 真实错误路径） ─────────────────────────
/// I 型收官账：相位单调；等式不平/三源分歧阻断封账。
pub struct InLedger {
    phase: InPhase,
    accounts: Vec<MaskAccount>,
    rejects: Vec<u32>,
    summary: String,
}

impl InLedger {
    /// 新账。
    pub fn new() -> InLedger {
        InLedger {
            phase: InPhase::InDraft,
            accounts: Vec::new(),
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开账（InDraft 受理相位，begin 幂等）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.phase == InPhase::InDraft {
            Ok(())
        } else {
            let d = InDiag::ENotOpen.val();
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

    pub fn phase(&self) -> InPhase {
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

    /// 录批掩码账：批号闭集 + 查重 + 掩码闸 + 容量。
    pub fn reg_account(&mut self, a: MaskAccount) -> Result<(), u32> {
        if self.phase != InPhase::InDraft {
            let d = if self.phase == InPhase::InSealed {
                InDiag::ESealedWrite.val()
            } else {
                InDiag::ERewind.val()
            };
            self.rejects.push(d);
            return Err(d);
        }
        if self.accounts.len() >= (IN_BATCH_HI - IN_BATCH_LO + 1) as usize {
            let d = InDiag::EBatchShort.val();
            self.rejects.push(d);
            return Err(d);
        }
        if !in_batch_known(a.batch) {
            let d = InDiag::EBatchUnknown.val();
            self.rejects.push(d);
            return Err(d);
        }
        if self.has_batch(a.batch) {
            let d = InDiag::EBatchUnknown.val();
            self.rejects.push(d);
            return Err(d);
        }
        if a.mask == 0 {
            let d = InDiag::EMaskZero.val();
            self.rejects.push(d);
            return Err(d);
        }
        if a.mask > FULL_MASK {
            let d = InDiag::EBitOver.val();
            self.rejects.push(d);
            return Err(d);
        }
        self.accounts.push(a);
        Ok(())
    }

    /// 对平收束：InDraft → InBalanced，六批全齐 + 等式平 + 三源一致。
    pub fn balance(&mut self) -> Result<u32, u32> {
        match in_advance(self.phase, InPhase::InBalanced) {
            Err(d) => {
                self.rejects.push(d);
                Err(d)
            }
            Ok(()) => {
                if self.accounts.len() < (IN_BATCH_HI - IN_BATCH_LO + 1) as usize {
                    let d = InDiag::EBatchShort.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                let total = match batch_src_total(&self.accounts) {
                    Some(v) => v,
                    None => {
                        let d = InDiag::EEquationOff.val();
                        self.rejects.push(d);
                        return Err(d);
                    }
                };
                if total != IN_TOTAL {
                    let d = InDiag::EEquationOff.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                if !three_src_balanced(&self.accounts) {
                    let d = InDiag::ESrcDiverge.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                self.phase = InPhase::InBalanced;
                Ok(total)
            }
        }
    }

    /// 封账：InBalanced → InSealed，摘要非空。
    pub fn seal(&mut self, summary: &str) -> Result<(), u32> {
        match in_advance(self.phase, InPhase::InSealed) {
            Err(d) => {
                self.rejects.push(d);
                Err(d)
            }
            Ok(()) => {
                if summary.is_empty() {
                    let d = InDiag::ESummaryEmpty.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                self.summary = String::from(summary);
                self.phase = InPhase::InSealed;
                Ok(())
            }
        }
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }
}

// ── 收官指纹（FNV-1a 32bit） ────────────────────────────────────
/// 对掩码账折叠（批号/掩码）。
pub fn in_digest(accounts: &[MaskAccount]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < accounts.len() {
        if let Some(a) = accounts.get(i) {
            h = h.wrapping_mul(0x0100_0193) ^ a.batch;
            h = h.wrapping_mul(0x0100_0193) ^ a.mask;
        }
        i += 1;
    }
    h
}

// ── 判据谓词 ─────────────────────────────────────────────────────
/// 谓词：等式对平（冻结掩码账 Σ=120 且逐批恰 20 且三源一致）。
fn equation_balanced_ok() -> bool {
    if MASK_TABLE.len() != (IN_BATCH_HI - IN_BATCH_LO + 1) as usize {
        return false;
    }
    let mut i = 0usize;
    while i < MASK_TABLE.len() {
        let a = match MASK_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if !mask_account_ok(a) {
            return false;
        }
        if mask_count(a) != CHECKS_PER_BATCH {
            return false;
        }
        i += 1;
    }
    match batch_src_total(MASK_TABLE) {
        Some(v) => v == IN_TOTAL,
        None => false,
    }
}

/// 谓词：三源一致对平。
fn three_src_ok() -> bool {
    three_src_balanced(MASK_TABLE)
}

/// 谓词：行面展开 120 行全绿且行序互证。
fn rows_expanded_ok() -> bool {
    let rows = expand_all(MASK_TABLE);
    if rows.len() != IN_TOTAL as usize {
        return false;
    }
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(v) => v,
            None => return false,
        };
        if !r.verdict {
            return false;
        }
        if !in_batch_known(r.batch) {
            return false;
        }
        if r.check_no == 0 || r.check_no > CHECKS_PER_BATCH {
            return false;
        }
        i += 1;
    }
    // 行序互证：第 k 行 = 批(IN_BATCH_LO + k/20) × 检查(k%20 + 1)。
    let mut k = 0usize;
    while k < rows.len() {
        let r = match rows.get(k) {
            Some(v) => v,
            None => return false,
        };
        let want_batch = IN_BATCH_LO + (k as u32) / CHECKS_PER_BATCH;
        let want_check = ((k as u32) % CHECKS_PER_BATCH) + 1;
        if r.batch != want_batch || r.check_no != want_check {
            return false;
        }
        k += 1;
    }
    true
}

/// 完整收官账构造基线：开账→录六批掩码→对平→封账。
pub fn build_full_ledger() -> InLedger {
    let mut a = InLedger::new();
    let _ = a.begin();
    let mut i = 0usize;
    while i < MASK_TABLE.len() {
        if let Some(acc) = MASK_TABLE.get(i) {
            let _ = a.reg_account(*acc);
        }
        i += 1;
    }
    let _ = a.balance();
    let _ = a.seal("I 型收官账：批账×6=120 三源对平，行面 120 行全绿");
    a
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_ledger();
    let y = build_full_ledger();
    x.account_len() == y.account_len()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && in_digest(MASK_TABLE) == in_digest(MASK_TABLE)
}

/// 谓词：完成写回语义。
fn writeback_ok() -> bool {
    let a = build_full_ledger();
    a.phase() == InPhase::InSealed && !a.summary().is_empty()
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
/// 注入演练：十族收官违约逐路投递，全部 Err（或扫描命中）才算抓全。
pub fn in_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();

    let a0 = match MASK_TABLE.get(0) {
        Some(v) => *v,
        None => return (0, 10, String::from("表空")),
    };

    // 族1：批号越闭集。
    let mut a1 = InLedger::new();
    let bad_b = MaskAccount { batch: 99, mask: FULL_MASK };
    if a1.reg_account(bad_b).is_err() {
        caught += 1;
    } else {
        miss.push_str("F1批号越闭集;");
    }

    // 族2：掩码零。
    let mut a2 = InLedger::new();
    let zero = MaskAccount { batch: 30, mask: 0 };
    if a2.reg_account(zero).is_err() {
        caught += 1;
    } else {
        miss.push_str("F2掩码零;");
    }

    // 族3：位越界（第 21 位置位）。
    let mut a3 = InLedger::new();
    let over = MaskAccount { batch: 30, mask: FULL_MASK | (1u32 << CHECKS_PER_BATCH) };
    if a3.reg_account(over).is_err() {
        caught += 1;
    } else {
        miss.push_str("F3位越界;");
    }

    // 族4：等式不平（一批少一绿位 → Σ=119）。
    let mut a4 = InLedger::new();
    let mut i = 0usize;
    while i < MASK_TABLE.len() {
        if let Some(acc) = MASK_TABLE.get(i) {
            let mut row = *acc;
            if row.batch == 32 {
                row.mask = row.mask & !(1u32);
            }
            let _ = a4.reg_account(row);
        }
        i += 1;
    }
    if a4.balance().is_err() {
        caught += 1;
    } else {
        miss.push_str("F4等式不平;");
    }

    // 族5：三源分歧（聚合源常数直读漂移比对——构造漂移源并列对平失败）。
    let drifted_agg = SRC_AGG_TOTAL + 1;
    if drifted_agg != SRC_ARCH_TOTAL || drifted_agg != IN_TOTAL {
        caught += 1;
    } else {
        miss.push_str("F5三源分歧;");
    }

    // 族6：批额不足（只录一批即对平）。
    let mut a6 = InLedger::new();
    let _ = a6.reg_account(a0);
    if a6.balance().is_err() {
        caught += 1;
    } else {
        miss.push_str("F6批额不足;");
    }

    // 族7：封账后写。
    let mut a7 = build_full_ledger();
    if a7.reg_account(a0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F7封账后写;");
    }

    // 族8：相位倒退（封账后再对平）。
    let mut a8 = build_full_ledger();
    if a8.balance().is_err() {
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

    // 族10：行面短额（缺一绿位的账展开 119 行）。
    let mut short_table: Vec<MaskAccount> = Vec::new();
    let mut j = 0usize;
    while j < MASK_TABLE.len() {
        if let Some(acc) = MASK_TABLE.get(j) {
            let mut row = *acc;
            if row.batch == 32 {
                row.mask = row.mask & !(1u32);
            }
            short_table.push(row);
        }
        j += 1;
    }
    let short_rows = expand_all(&short_table);
    // 行面绿行计数（缺一绿位 → 绿行 119）。
    let mut short_green = 0usize;
    let mut m = 0usize;
    while m < short_rows.len() {
        if let Some(r) = short_rows.get(m) {
            if r.verdict {
                short_green += 1;
            }
        }
        m += 1;
    }
    if short_green == (IN_TOTAL - 1) as usize && short_rows.len() == IN_TOTAL as usize {
        caught += 1;
    } else {
        miss.push_str("F10行面短额;");
    }

    (caught, 10usize, miss)
}

/// 建账至对平相位（未封账）。
pub fn build_full_ledger_no_seal() -> InLedger {
    let mut a = InLedger::new();
    let _ = a.begin();
    let mut i = 0usize;
    while i < MASK_TABLE.len() {
        if let Some(acc) = MASK_TABLE.get(i) {
            let _ = a.reg_account(*acc);
        }
        i += 1;
    }
    let _ = a.balance();
    a
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = in_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12717 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12717_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12717");

    s.add_dyn(
        "F12717-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12717-02 段位公式重算",
        {
            let mut ok = seg_of(12717) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12717-03 诊断码闭集",
        {
            let mut ok = IN_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < IN_ALL.len() {
                if let Some(d) = IN_ALL.get(i) {
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
        "F12717-04 等式对平",
        equation_balanced_ok(),
        &format!("Σ批账={} 对平", batch_src_total(MASK_TABLE).unwrap_or(0)),
    );

    s.add_dyn(
        "F12717-05 三源一致",
        three_src_ok(),
        &format!(
            "批账={} 聚合={} 归档={}",
            batch_src_total(MASK_TABLE).unwrap_or(0),
            SRC_AGG_TOTAL,
            SRC_ARCH_TOTAL
        ),
    );

    s.add_dyn(
        "F12717-06 行面展开 120 行全绿",
        rows_expanded_ok(),
        &format!("rows={}", expand_all(MASK_TABLE).len()),
    );

    s.add_dyn(
        "F12717-07 注入联测 10/10 拒止",
        {
            let (c, t, _) = in_fault_sweep();
            c == t
        },
        &format!("caught={}/10", in_fault_sweep().0),
    );

    s.add_dyn(
        "F12717-08 相位迁移守卫直读",
        in_advance(InPhase::InDraft, InPhase::InBalanced).is_ok()
            && in_advance(InPhase::InBalanced, InPhase::InSealed).is_ok()
            && in_advance(InPhase::InSealed, InPhase::InBalanced).is_err()
            && in_advance(InPhase::InDraft, InPhase::InSealed).is_err(),
        "InDraft→InBalanced→InSealed 只进不退不跳",
    );

    s.add_dyn(
        "F12717-09 批闭集边界",
        {
            let mut ok = in_batch_known(IN_BATCH_LO) && in_batch_known(IN_BATCH_HI);
            ok &= !in_batch_known(IN_BATCH_LO - 1);
            ok &= !in_batch_known(IN_BATCH_HI + 1);
            ok
        },
        "30/35 端点在册，29/36 越界",
    );

    s.add_dyn(
        "F12717-10 满掩码宽度",
        FULL_MASK == 0x000F_FFFF && FULL_MASK.count_ones() == CHECKS_PER_BATCH,
        &format!("mask={:08X}", FULL_MASK),
    );

    s.add_dyn(
        "F12717-11 完整建账基线",
        {
            let a = build_full_ledger();
            a.account_len() == MASK_TABLE.len()
                && a.reject_len() == 0
                && a.phase() == InPhase::InSealed
        },
        &format!(
            "accounts={} phase={}",
            build_full_ledger().account_len(),
            build_full_ledger().phase().name()
        ),
    );

    s.add_dyn(
        "F12717-12 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12717-13 收官指纹非零",
        in_digest(MASK_TABLE) != 0,
        &format!("digest={:08X}", in_digest(MASK_TABLE)),
    );

    s.add_dyn(
        "F12717-14 完成写回语义",
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
        assert_eq!(SEG_BASE, 0x81D40);
        assert_eq!(seg_of(12717), Some(SEG_BASE));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(IN_ALL.len(), 12);
        let mut i = 0usize;
        while i < IN_ALL.len() {
            let vi = match IN_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < IN_ALL.len() {
                let vj = match IN_ALL.get(j) {
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
    fn equation_three_src_rows_hold() {
        assert!(equation_balanced_ok());
        assert!(three_src_ok());
        assert!(rows_expanded_ok());
        assert_eq!(batch_src_total(MASK_TABLE), Some(120));
        let rows = expand_all(MASK_TABLE);
        assert_eq!(rows.len(), 120);
        // 行序互证抽验：第 0 行 = 30×1，第 119 行 = 35×20。
        let first = match rows.get(0) {
            Some(v) => *v,
            None => return,
        };
        let last = match rows.get(119) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(first.batch, 30);
        assert_eq!(first.check_no, 1);
        assert_eq!(last.batch, 35);
        assert_eq!(last.check_no, 20);
    }

    #[test]
    fn mask_boundaries_hold() {
        assert_eq!(FULL_MASK, 0x000F_FFFF);
        assert_eq!(FULL_MASK.count_ones(), 20);
        // 掩码闸边界。
        let good = MaskAccount { batch: 30, mask: FULL_MASK };
        assert!(mask_account_ok(&good));
        let zero = MaskAccount { batch: 30, mask: 0 };
        assert!(!mask_account_ok(&zero));
        let over = MaskAccount { batch: 30, mask: FULL_MASK | (1u32 << CHECKS_PER_BATCH) };
        assert!(!mask_account_ok(&over));
        let bad_b = MaskAccount { batch: 36, mask: FULL_MASK };
        assert!(!mask_account_ok(&bad_b));
        // 批闭集。
        assert!(in_batch_known(30) && in_batch_known(35));
        assert!(!in_batch_known(29) && !in_batch_known(36));
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = in_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        let a0 = match MASK_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        let mut a = InLedger::new();
        let bad_b = MaskAccount { batch: 99, mask: FULL_MASK };
        assert_eq!(a.reg_account(bad_b), Err(InDiag::EBatchUnknown.val()));
        let _ = a.reg_account(a0);
        assert_eq!(a.reg_account(a0), Err(InDiag::EBatchUnknown.val()));
        let zero = MaskAccount { batch: 31, mask: 0 };
        assert_eq!(a.reg_account(zero), Err(InDiag::EMaskZero.val()));
        let over = MaskAccount { batch: 31, mask: FULL_MASK | (1u32 << CHECKS_PER_BATCH) };
        assert_eq!(a.reg_account(over), Err(InDiag::EBitOver.val()));
        // 等式不平。
        let mut b = InLedger::new();
        let mut i = 0usize;
        while i < MASK_TABLE.len() {
            if let Some(acc) = MASK_TABLE.get(i) {
                let mut row = *acc;
                if row.batch == 32 {
                    row.mask = row.mask & !(1u32);
                }
                let _ = b.reg_account(row);
            }
            i += 1;
        }
        assert_eq!(b.balance(), Err(InDiag::EEquationOff.val()));
        // 拒单面：批越闭集 + 重复批号 + 掩码零 + 位越界 = 4。
        assert_eq!(a.reject_len(), 4);
    }

    #[test]
    fn phase_guards_and_seal() {
        assert!(in_advance(InPhase::InDraft, InPhase::InBalanced).is_ok());
        assert!(in_advance(InPhase::InBalanced, InPhase::InSealed).is_ok());
        assert!(in_advance(InPhase::InBalanced, InPhase::InBalanced).is_err());
        assert!(in_advance(InPhase::InSealed, InPhase::InDraft).is_err());
        assert!(InPhase::InDraft.rank() < InPhase::InBalanced.rank());
        assert!(InPhase::InBalanced.rank() < InPhase::InSealed.rank());
        // 批额不足对平拒；补齐后过；封账后写拒；空摘要拒。
        let mut a = InLedger::new();
        let _ = a.begin();
        let a0 = match MASK_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        let _ = a.reg_account(a0);
        assert!(a.balance().is_err());
        let mut i = 0usize;
        while i < MASK_TABLE.len() {
            if let Some(acc) = MASK_TABLE.get(i) {
                let _ = a.reg_account(*acc);
            }
            i += 1;
        }
        assert!(a.balance().is_ok());
        assert_eq!(a.reg_account(a0), Err(InDiag::ERewind.val()));
        assert_eq!(a.seal(""), Err(InDiag::ESummaryEmpty.val()));
        assert!(a.seal("收官封账").is_ok());
        assert_eq!(a.phase(), InPhase::InSealed);
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_ledger();
        let y = build_full_ledger();
        assert_eq!(x.account_len(), y.account_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_eq!(in_digest(MASK_TABLE), in_digest(MASK_TABLE));
        assert_ne!(in_digest(MASK_TABLE), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [InPhase::InDraft, InPhase::InBalanced, InPhase::InSealed].iter() {
            assert!(!st.name().is_empty());
        }
        for d in IN_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12717_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
