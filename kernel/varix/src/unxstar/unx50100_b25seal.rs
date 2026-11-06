//! UNX-F50100 B25 批收官印（20 条冻结 / 累计 150,000 行）（0x438a0 段）
//!
//! 锚点：UNX-F50100-J1 批印入册：20/20 冻结、域账 150,000/240,000；判据锚定 §AI-63 §7。
//!
//! 数据段：`SEG_BASE = 0x438a0`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 17`，
//! 逐项见 [`SealError`]。
//!
//! 收官印不是常数表，而是「装订表 → 冻结 → 里程碑对齐 → 域账 → 印面」五段
//! 流水线。与 B24 的差别在于多了一张**里程碑表**：单批数字对得上不代表累计
//! 曲线对得上，必须把 B21..B25 五批的域账一起拉平，批印才允许落地。
//!
//! ```text
//! PLAN(20 条) --load--> SealLedger(entries)
//!                           |--freeze_all--> 20/20 frozen
//!                           |--milestones--> B21..B25 等差对齐
//!                           +--seal--> SealStamp(digest) --audit--> Ok
//! ```
//!
//! 域账口径（可复算）：`域账行 = Σ卡面配额 × 批序数`。B25 二十条卡面配额合计
//! 6,000 行，×25 = **150,000**，与 B24 的 6,000×24 = 144,000 构成步长 6,000
//! 的等差数列；预算 240,000 是 B21–B30 的域上限，本批占 62.5%（625‰）。
//!
//! 冻结边界取 `[配额, 8×配额]`：B25 实做密度约 3~5 倍（参考 F50001 为
//! 1,400/420 ≈ 3.3），高于 8 倍视为灌水，低于配额视为没写完——两头都显式拒绝。
//!
//! 结构清单：
//! * [`PlanRec`] / [`PLAN`]：批次装订表（卡号 / slug / 配额 / 判据数）；
//! * [`MilestoneRec`] / [`MILESTONES`] / [`MilestoneTable`]：B21..B25 累计里程碑；
//! * [`SealEntry`] / [`SealLedger`]：定长账本（装载 / 冻结 / 落印 / 审计）；
//! * [`SealStamp`] / [`AccountView`]：印面与域账视图；
//! * [`injection_core`]：17 类坏样本注入核（逐类断言具体诊断码）。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x438a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x438a0;

const E_EMPTY: u16 = 1;
const E_SIZE: u16 = 2;
const E_NUM: u16 = 3;
const E_QUOTA: u16 = 4;
const E_CHECKS: u16 = 5;
const E_DUPLICATE: u16 = 6;
const E_ORDER: u16 = 7;
const E_UNKNOWN: u16 = 8;
const E_ALREADY_FROZEN: u16 = 9;
const E_NOT_FROZEN: u16 = 10;
const E_ACCOUNT: u16 = 11;
const E_BUDGET: u16 = 12;
const E_ALREADY_SEALED: u16 = 13;
const E_IMMUTABLE: u16 = 14;
const E_DIGEST: u16 = 15;
const E_UNSEALED: u16 = 16;
const E_MILESTONE: u16 = 17;

/// 收官印错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealError {
    /// 装订表 / 里程碑表为空。
    Empty,
    /// 条数不等于 20。
    Size,
    /// 卡号越出 B25 区间。
    Num,
    /// 配额为 0，或账面行数不在 `[配额, 8×配额]`。
    Quota,
    /// 判据数越出 8..=20。
    Checks,
    /// 卡号重复。
    Duplicate,
    /// 卡号未升序。
    Order,
    /// 卡号不在账本里。
    Unknown,
    /// 该条已冻结。
    AlreadyFrozen,
    /// 尚有未冻结条目。
    NotFrozen,
    /// 域账未命中检查点（或里程碑末项与账面不一致）。
    Account,
    /// 域账或里程碑超出预算。
    Budget,
    /// 已落印，不能重复落印。
    AlreadySealed,
    /// 已落印，整本只读。
    Immutable,
    /// 印面摘要与账本不符（被篡改）。
    Digest,
    /// 尚未落印，无印可审。
    Unsealed,
    /// 里程碑表自相矛盾（非升序 / 折算不成立）。
    Milestone,
}

impl SealError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            SealError::Empty => E_EMPTY,
            SealError::Size => E_SIZE,
            SealError::Num => E_NUM,
            SealError::Quota => E_QUOTA,
            SealError::Checks => E_CHECKS,
            SealError::Duplicate => E_DUPLICATE,
            SealError::Order => E_ORDER,
            SealError::Unknown => E_UNKNOWN,
            SealError::AlreadyFrozen => E_ALREADY_FROZEN,
            SealError::NotFrozen => E_NOT_FROZEN,
            SealError::Account => E_ACCOUNT,
            SealError::Budget => E_BUDGET,
            SealError::AlreadySealed => E_ALREADY_SEALED,
            SealError::Immutable => E_IMMUTABLE,
            SealError::Digest => E_DIGEST,
            SealError::Unsealed => E_UNSEALED,
            SealError::Milestone => E_MILESTONE,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断（日志与串口共用，不申请内存）。
    pub fn message(self) -> &'static str {
        match self {
            SealError::Empty => "batch plan or milestone table empty",
            SealError::Size => "batch does not have 20 cards",
            SealError::Num => "card number outside B25 window",
            SealError::Quota => "quota zero or lines outside [quota, 8*quota]",
            SealError::Checks => "check count outside 8..=20",
            SealError::Duplicate => "card number registered twice",
            SealError::Order => "card numbers not ascending",
            SealError::Unknown => "card number not in ledger",
            SealError::AlreadyFrozen => "entry already frozen",
            SealError::NotFrozen => "entries still unfrozen",
            SealError::Account => "domain account misses checkpoint 150000",
            SealError::Budget => "domain account exceeds budget 240000",
            SealError::AlreadySealed => "batch already sealed",
            SealError::Immutable => "ledger sealed read-only",
            SealError::Digest => "seal digest mismatch",
            SealError::Unsealed => "batch is not sealed yet",
            SealError::Milestone => "milestone table inconsistent",
        }
    }
}

/// B25 批首卡号。
pub const NUM_FIRST: u32 = 50081;
/// B25 批末卡号。
pub const NUM_LAST: u32 = 50100;
/// 本批条数。
pub const BATCH_SIZE: usize = 20;
/// 域账折算系数：卡面配额 × 批序数（B25 即 ×25）。
pub const DOMAIN_FACTOR: u32 = 25;
/// 本批域账检查点：150,000。
pub const DOMAIN_CHECKPOINT: u32 = 150_000;
/// B21–B30 域账预算。
pub const DOMAIN_BUDGET: u32 = 240_000;
/// 判据数下限。
pub const CHECKS_MIN: u8 = 8;
/// 判据数上限。
pub const CHECKS_MAX: u8 = 20;
/// 冻结上限倍数（见模块头：3~5 倍是实做密度，8 倍起视为灌水）。
pub const QUOTA_SPREAD: u32 = 8;

/// 装订表条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanRec {
    /// 卡号。
    pub num: u32,
    /// 文件 slug。
    pub slug: &'static str,
    /// 卡面行数配额。
    pub quota: u32,
    /// 判据条数（8..=20）。
    pub checks: u8,
}

/// B25 装订表：20 条，配额取自计划表 `lines` 字段，合计 6,000。
pub const PLAN: [PlanRec; BATCH_SIZE] = [
    PlanRec { num: 50081, slug: "unx50081_subsetter", quota: 420, checks: 14 },
    PlanRec { num: 50082, slug: "unx50082_fontembed", quota: 480, checks: 16 },
    PlanRec { num: 50083, slug: "unx50083_residentmatch", quota: 400, checks: 14 },
    PlanRec { num: 50084, slug: "unx50084_fontdl", quota: 380, checks: 12 },
    PlanRec { num: 50085, slug: "unx50085_cjkdl", quota: 360, checks: 12 },
    PlanRec { num: 50086, slug: "unx50086_fontlicense", quota: 340, checks: 12 },
    PlanRec { num: 50087, slug: "unx50087_fontsubst", quota: 320, checks: 12 },
    PlanRec { num: 50088, slug: "unx50088_fontmetrics", quota: 300, checks: 12 },
    PlanRec { num: 50089, slug: "unx50089_fontgdi", quota: 300, checks: 10 },
    PlanRec { num: 50090, slug: "unx50090_fonttrace", quota: 280, checks: 10 },
    PlanRec { num: 50091, slug: "unx50091_fonterrs", quota: 280, checks: 12 },
    PlanRec { num: 50092, slug: "unx50092_fontfuzz", quota: 260, checks: 10 },
    PlanRec { num: 50093, slug: "unx50093_fontsamples", quota: 260, checks: 10 },
    PlanRec { num: 50094, slug: "unx50094_fontfreeze", quota: 240, checks: 10 },
    PlanRec { num: 50095, slug: "unx50095_fontvec", quota: 240, checks: 10 },
    PlanRec { num: 50096, slug: "unx50096_fontiso", quota: 220, checks: 10 },
    PlanRec { num: 50097, slug: "unx50097_fontt1", quota: 220, checks: 15 },
    PlanRec { num: 50098, slug: "unx50098_b25vec", quota: 200, checks: 14 },
    PlanRec { num: 50099, slug: "unx50099_b25grep", quota: 200, checks: 14 },
    PlanRec { num: 50100, slug: "unx50100_b25seal", quota: 300, checks: 16 },
];

/// 一批的里程碑：卡面行数合计 × 批序数 = 该批域账已用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MilestoneRec {
    /// 批序数（21..=25）。
    pub batch: u8,
    /// 该批卡面行数合计。
    pub card_lines: u32,
    /// 折算系数（等于 `batch`）。
    pub factor: u32,
    /// 域账已用（`card_lines × factor`）。
    pub used: u32,
}

/// B21..B25 里程碑：每批 6,000 卡面行，折算系数随批序数递增，
/// 因此域账构成 126,000 / 132,000 / 138,000 / 144,000 / 150,000 的等差数列。
pub const MILESTONES: [MilestoneRec; 5] = [
    MilestoneRec { batch: 21, card_lines: 6_000, factor: 21, used: 126_000 },
    MilestoneRec { batch: 22, card_lines: 6_000, factor: 22, used: 132_000 },
    MilestoneRec { batch: 23, card_lines: 6_000, factor: 23, used: 138_000 },
    MilestoneRec { batch: 24, card_lines: 6_000, factor: 24, used: 144_000 },
    MilestoneRec { batch: 25, card_lines: 6_000, factor: 25, used: 150_000 },
];

/// 账本条目：装订信息 + 冻结结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SealEntry {
    /// 卡号。
    pub num: u32,
    /// 文件 slug。
    pub slug: &'static str,
    /// 卡面配额。
    pub quota: u32,
    /// 账面行数（冻结时写入）。
    pub lines: u32,
    /// 判据条数。
    pub checks: u8,
    /// 是否已冻结。
    pub frozen: bool,
}

/// 印面。落印后随账本只读。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SealStamp {
    /// 冻结条数（应为 20）。
    pub count: u32,
    /// 配额合计（6,000）。
    pub quota_sum: u32,
    /// 账面行数合计（6,000）。
    pub lines_sum: u32,
    /// 域账已用（lines_sum × 25 = 150,000）。
    pub domain_used: u32,
    /// 域账预算。
    pub budget: u32,
    /// 里程碑末项。
    pub milestone_used: u32,
    /// 全账 FNV-1a 摘要。
    pub digest: u32,
}

/// 域账视图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccountView {
    /// 已用域账行。
    pub used: u32,
    /// 预算。
    pub budget: u32,
    /// 占用千分比（625 = 62.5%）。
    pub permille: u32,
}

/// 里程碑表：装载时逐项复核折算与单调性，末项必须落在本批检查点上。
#[derive(Clone, Debug)]
pub struct MilestoneTable {
    recs: Vec<MilestoneRec>,
}

impl MilestoneTable {
    /// 装载里程碑表。
    ///
    /// 校验顺序：空表 → 批序数连续升序 → 折算恒等式 → 单调递增 → 预算 →
    /// 末项 == 150,000。这个顺序保证同一个坏表只报「第一个真正的毛病」。
    pub fn load(recs: &[MilestoneRec]) -> Result<MilestoneTable, SealError> {
        if recs.is_empty() {
            return Err(SealError::Empty);
        }
        let mut out = Vec::with_capacity(recs.len());
        let mut prev_used = 0u32;
        let mut i = 0usize;
        while i < recs.len() {
            let r = match recs.get(i) {
                Some(v) => *v,
                None => return Err(SealError::Empty),
            };
            if r.factor == 0 || r.factor != r.batch as u32 {
                return Err(SealError::Milestone);
            }
            if i == 0 {
                if r.batch != 21 {
                    return Err(SealError::Milestone);
                }
            } else if r.batch as u32 != 21 + i as u32 || r.batch == 0 {
                return Err(SealError::Milestone);
            }
            let want = r.card_lines.saturating_mul(r.factor);
            if want != r.used {
                return Err(SealError::Milestone);
            }
            if i > 0 && r.used <= prev_used {
                return Err(SealError::Milestone);
            }
            if r.used > DOMAIN_BUDGET {
                return Err(SealError::Budget);
            }
            prev_used = r.used;
            out.push(r);
            i += 1;
        }
        match out.last() {
            Some(last) => {
                if last.used != DOMAIN_CHECKPOINT {
                    return Err(SealError::Account);
                }
            }
            None => return Err(SealError::Empty),
        }
        Ok(MilestoneTable { recs: out })
    }

    /// 按正式里程碑表装载。
    pub fn from_builtin() -> Result<MilestoneTable, SealError> {
        MilestoneTable::load(&MILESTONES)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 空表判定。
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    /// 读一条。
    pub fn at(&self, idx: usize) -> Option<MilestoneRec> {
        self.recs.get(idx).copied()
    }

    /// 末项域账。
    pub fn last_used(&self) -> u32 {
        match self.recs.last() {
            Some(r) => r.used,
            None => 0,
        }
    }
}

/// B25 批账本。
#[derive(Clone, Debug)]
pub struct SealLedger {
    entries: Vec<SealEntry>,
    milestones: MilestoneTable,
    sealed: bool,
    stamp: Option<SealStamp>,
}

fn push_fnv(h: u32, v: u32) -> u32 {
    const PRIME: u32 = 0x0100_0193;
    let bytes = [(v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8];
    let mut out = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            out = (out ^ *b as u32).wrapping_mul(PRIME);
        }
        i += 1;
    }
    out
}

impl SealLedger {
    /// 从装订表装载账本（里程碑取内置表）。
    pub fn load(recs: &[PlanRec]) -> Result<SealLedger, SealError> {
        let ms = MilestoneTable::from_builtin()?;
        SealLedger::with_milestones(recs, &ms)
    }

    /// 从装订表 + 指定里程碑表装载（跨批导入时用）。
    pub fn with_milestones(recs: &[PlanRec], milestones: &MilestoneTable) -> Result<SealLedger, SealError> {
        if recs.is_empty() {
            return Err(SealError::Empty);
        }
        if recs.len() != BATCH_SIZE {
            return Err(SealError::Size);
        }
        let mut entries = Vec::with_capacity(BATCH_SIZE);
        let mut prev = 0u32;
        let mut i = 0usize;
        while i < recs.len() {
            let r = match recs.get(i) {
                Some(v) => v,
                None => return Err(SealError::Empty),
            };
            if r.num < NUM_FIRST || r.num > NUM_LAST {
                return Err(SealError::Num);
            }
            if r.quota == 0 {
                return Err(SealError::Quota);
            }
            if r.checks < CHECKS_MIN || r.checks > CHECKS_MAX {
                return Err(SealError::Checks);
            }
            if i > 0 && r.num == prev {
                return Err(SealError::Duplicate);
            }
            if i > 0 && r.num < prev {
                return Err(SealError::Order);
            }
            entries.push(SealEntry {
                num: r.num,
                slug: r.slug,
                quota: r.quota,
                lines: 0,
                checks: r.checks,
                frozen: false,
            });
            prev = r.num;
            i += 1;
        }
        Ok(SealLedger {
            entries,
            milestones: milestones.clone(),
            sealed: false,
            stamp: None,
        })
    }

    /// 按正式装订表装载。
    pub fn from_plan() -> Result<SealLedger, SealError> {
        SealLedger::load(&PLAN)
    }

    /// 冻结一条：账面行数必须落在 `[配额, 8×配额]`。
    pub fn freeze(&mut self, num: u32, lines: u32) -> Result<(), SealError> {
        if self.sealed {
            return Err(SealError::Immutable);
        }
        let mut at = None;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.num == num {
                    at = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let idx = match at {
            Some(v) => v,
            None => return Err(SealError::Unknown),
        };
        if let Some(e) = self.entries.get(idx) {
            if e.frozen {
                return Err(SealError::AlreadyFrozen);
            }
            let hi = e.quota.saturating_mul(QUOTA_SPREAD);
            if lines < e.quota || lines > hi {
                return Err(SealError::Quota);
            }
        }
        if let Some(e) = self.entries.get_mut(idx) {
            e.lines = lines;
            e.frozen = true;
        }
        Ok(())
    }

    /// 按装订表逐条冻结为「恰好配额」。
    pub fn freeze_all(&mut self) -> Result<u32, SealError> {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            let pair = match self.entries.get(i) {
                Some(e) => (e.num, e.quota),
                None => break,
            };
            self.freeze(pair.0, pair.1)?;
            n += 1;
            i += 1;
        }
        Ok(n)
    }

    /// 已冻结条数。
    pub fn frozen_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.frozen {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 配额合计。
    pub fn quota_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                n += e.quota;
            }
            i += 1;
        }
        n
    }

    /// 账面行数合计。
    pub fn lines_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                n += e.lines;
            }
            i += 1;
        }
        n
    }

    /// 域账已用行数。
    pub fn domain_used(&self) -> u32 {
        self.lines_sum().saturating_mul(DOMAIN_FACTOR)
    }

    /// 域账视图（千分比按整数除法，预算为 0 时返回 0，不除零）。
    pub fn domain(&self) -> AccountView {
        let used = self.domain_used();
        let permille = if DOMAIN_BUDGET == 0 {
            0
        } else {
            (used as u64 * 1000 / DOMAIN_BUDGET as u64) as u32
        };
        AccountView { used, budget: DOMAIN_BUDGET, permille }
    }

    /// 全账摘要：逐条折叠卡号 / 配额 / 行数 / 判据数 / 冻结位。
    pub fn digest(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = push_fnv(h, e.num);
                h = push_fnv(h, e.quota);
                h = push_fnv(h, e.lines);
                h = push_fnv(h, e.checks as u32);
                h = push_fnv(h, e.frozen as u32);
            }
            i += 1;
        }
        h
    }

    /// 落印：条数 → 冻结 → 预算 → 检查点 → 里程碑对齐，随后整本只读。
    pub fn seal(&mut self) -> Result<SealStamp, SealError> {
        if self.sealed {
            return Err(SealError::AlreadySealed);
        }
        if self.entries.len() != BATCH_SIZE {
            return Err(SealError::Size);
        }
        if self.frozen_count() != BATCH_SIZE as u32 {
            return Err(SealError::NotFrozen);
        }
        let used = self.domain_used();
        if used > DOMAIN_BUDGET {
            return Err(SealError::Budget);
        }
        if used != DOMAIN_CHECKPOINT {
            return Err(SealError::Account);
        }
        if self.milestones.last_used() != used {
            return Err(SealError::Milestone);
        }
        let stamp = SealStamp {
            count: self.frozen_count(),
            quota_sum: self.quota_sum(),
            lines_sum: self.lines_sum(),
            domain_used: used,
            budget: DOMAIN_BUDGET,
            milestone_used: self.milestones.last_used(),
            digest: self.digest(),
        };
        self.stamp = Some(stamp);
        self.sealed = true;
        Ok(stamp)
    }

    /// 审计：印面必须在，摘要 / 条数 / 域账 / 里程碑必须逐项一致。
    pub fn audit(&self) -> Result<(), SealError> {
        let stamp = match self.stamp {
            Some(v) => v,
            None => return Err(SealError::Unsealed),
        };
        if stamp.digest != self.digest() {
            return Err(SealError::Digest);
        }
        if stamp.count != self.frozen_count() || stamp.domain_used != self.domain_used() {
            return Err(SealError::Digest);
        }
        if stamp.milestone_used != self.milestones.last_used() {
            return Err(SealError::Milestone);
        }
        Ok(())
    }

    /// 是否已落印。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 当前印面。
    pub fn stamp(&self) -> Option<SealStamp> {
        self.stamp
    }

    /// 读一条账目。
    pub fn entry(&self, idx: usize) -> Option<SealEntry> {
        self.entries.get(idx).copied()
    }

    /// 账本条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 里程碑表引用。
    pub fn milestones(&self) -> &MilestoneTable {
        &self.milestones
    }

    /// 一行报告（结果摘要非空，供软件写回）。
    pub fn report(&self) -> String {
        let acct = self.domain();
        format!(
            "B25 seal frozen={}/{} quota={} lines={} domain={}/{} permille={} milestones={} sealed={}",
            self.frozen_count(),
            BATCH_SIZE,
            self.quota_sum(),
            self.lines_sum(),
            acct.used,
            acct.budget,
            acct.permille,
            self.milestones.len(),
            self.sealed as u8
        )
    }
}

impl Default for SealLedger {
    fn default() -> SealLedger {
        // 装订表与里程碑表都是编译期常量，正常不可能失败；失败时退化为空账而非 panic。
        match SealLedger::from_plan() {
            Ok(v) => v,
            Err(_) => SealLedger {
                entries: Vec::new(),
                milestones: MilestoneTable { recs: Vec::new() },
                sealed: false,
                stamp: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

fn plan_vec() -> Vec<PlanRec> {
    let mut v = Vec::with_capacity(PLAN.len());
    let mut i = 0usize;
    while i < PLAN.len() {
        if let Some(r) = PLAN.get(i) {
            v.push(*r);
        }
        i += 1;
    }
    v
}

fn full_ledger() -> SealLedger {
    match SealLedger::from_plan() {
        Ok(v) => v,
        Err(_) => SealLedger::default(),
    }
}

fn empty_ledger() -> SealLedger {
    SealLedger {
        entries: Vec::new(),
        milestones: MilestoneTable { recs: Vec::new() },
        sealed: false,
        stamp: None,
    }
}

/// 坏样本注入核：17 类错误必须逐类命中具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 17usize;

    // 1 空表
    if SealLedger::load(&[]).err() == Some(SealError::Empty) {
        caught += 1;
    }
    // 2 条数不足
    if SealLedger::load(&PLAN[0..BATCH_SIZE - 1]).err() == Some(SealError::Size) {
        caught += 1;
    }
    // 3 卡号越界
    let mut bad_num = plan_vec();
    if let Some(r) = bad_num.get_mut(0) {
        r.num = 1;
    }
    if SealLedger::load(&bad_num).err() == Some(SealError::Num) {
        caught += 1;
    }
    // 4 配额为 0
    let mut bad_quota = plan_vec();
    if let Some(r) = bad_quota.get_mut(0) {
        r.quota = 0;
    }
    if SealLedger::load(&bad_quota).err() == Some(SealError::Quota) {
        caught += 1;
    }
    // 5 判据数越界
    let mut bad_checks = plan_vec();
    if let Some(r) = bad_checks.get_mut(0) {
        r.checks = 4;
    }
    if SealLedger::load(&bad_checks).err() == Some(SealError::Checks) {
        caught += 1;
    }
    // 6 卡号重复
    let mut dup = plan_vec();
    let first = PLAN.get(0).copied();
    if let (Some(a), Some(b)) = (dup.get_mut(1), first) {
        *a = b;
    }
    if SealLedger::load(&dup).err() == Some(SealError::Duplicate) {
        caught += 1;
    }
    // 7 卡号倒序
    let mut swapped = plan_vec();
    let v0 = swapped.get(0).copied();
    let v1 = swapped.get(1).copied();
    if let (Some(a), Some(b)) = (v0, v1) {
        if let Some(s) = swapped.get_mut(0) {
            *s = b;
        }
        if let Some(s) = swapped.get_mut(1) {
            *s = a;
        }
    }
    if SealLedger::load(&swapped).err() == Some(SealError::Order) {
        caught += 1;
    }
    // 8 未知卡号
    let mut led = full_ledger();
    if led.freeze(9999, 100).err() == Some(SealError::Unknown) {
        caught += 1;
    }
    // 9 重复冻结
    let _ = led.freeze(NUM_FIRST, 420);
    if led.freeze(NUM_FIRST, 420).err() == Some(SealError::AlreadyFrozen) {
        caught += 1;
    }
    // 10 未全冻结落印
    if led.seal().err() == Some(SealError::NotFrozen) {
        caught += 1;
    }
    // 11 域账未命中检查点（多 1 行 → 150,025）
    let mut off = full_ledger();
    let _ = off.freeze_all();
    let base = match off.entries.get(0) {
        Some(e) => e.lines,
        None => 0,
    };
    if let Some(e) = off.entries.get_mut(0) {
        e.lines = base + 1;
    }
    if off.seal().err() == Some(SealError::Account) {
        caught += 1;
    }
    // 12 域账超预算（全部按 8×配额冻结 → 1,200,000）
    let mut over = full_ledger();
    let mut i = 0usize;
    while i < over.len() {
        let pair = match over.entries.get(i) {
            Some(e) => (e.num, e.quota),
            None => break,
        };
        let _ = over.freeze(pair.0, pair.1.saturating_mul(QUOTA_SPREAD));
        i += 1;
    }
    if over.seal().err() == Some(SealError::Budget) {
        caught += 1;
    }
    // 13 重复落印
    let mut sealed = full_ledger();
    let _ = sealed.freeze_all();
    let _ = sealed.seal();
    if sealed.seal().err() == Some(SealError::AlreadySealed) {
        caught += 1;
    }
    // 14 印后冻结
    if sealed.freeze(NUM_LAST, 300).err() == Some(SealError::Immutable) {
        caught += 1;
    }
    // 15 印面篡改
    let mut tampered = full_ledger();
    let _ = tampered.freeze_all();
    let _ = tampered.seal();
    if let Some(e) = tampered.entries.get_mut(3) {
        e.lines = e.lines.saturating_add(1);
    }
    if tampered.audit().err() == Some(SealError::Digest) {
        caught += 1;
    }
    // 16 未落印先审计（空账同样必须是 Unsealed，不能倒过来报 Account）
    let fresh = empty_ledger();
    if fresh.audit().err() == Some(SealError::Unsealed) {
        caught += 1;
    }
    // 17 里程碑表非单调（第 3 项被改小）
    let mut ms = Vec::with_capacity(MILESTONES.len());
    let mut k = 0usize;
    while k < MILESTONES.len() {
        if let Some(m) = MILESTONES.get(k).copied() {
            ms.push(m);
        }
        k += 1;
    }
    if let Some(m) = ms.get_mut(2) {
        m.used = 1_000;
        m.card_lines = 1_000 / m.factor.max(1);
    }
    if MilestoneTable::load(&ms).err() == Some(SealError::Milestone) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 跑 UNX-F50100 的 16 条判据（含 J1 批印入册与注入核）。
pub fn run_unx50100_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50100");

    // J1-1 装订表自洽：20 条、卡号 50081..=50100 升序、slug 非空。
    let mut ok = PLAN.len() == BATCH_SIZE;
    let mut i = 0usize;
    while i < PLAN.len() {
        if let Some(r) = PLAN.get(i) {
            if r.num != NUM_FIRST + i as u32 || r.slug.is_empty() {
                ok = false;
            }
            if r.checks < CHECKS_MIN || r.checks > CHECKS_MAX {
                ok = false;
            }
        }
        i += 1;
    }
    let ok = ok && PLAN.get(BATCH_SIZE - 1).map(|r| r.num) == Some(NUM_LAST);
    s.add("50100-J1-装订表", ok, if ok { "" } else { "装订表不自洽" });

    // J1-2 配额账：Σquota = 6,000，×25 = 150,000（域账检查点）。
    let mut q = 0u32;
    let mut j = 0usize;
    while j < PLAN.len() {
        if let Some(r) = PLAN.get(j) {
            q += r.quota;
        }
        j += 1;
    }
    let ok = q == 6_000 && q * DOMAIN_FACTOR == DOMAIN_CHECKPOINT;
    let det = format!("quota={} x{}={}", q, DOMAIN_FACTOR, q * DOMAIN_FACTOR);
    s.add_dyn("50100-J1-配额账", ok, &det);

    // J1-3 里程碑表：五批等差 6,000、严格升序、末项 == 150,000。
    let ms = MilestoneTable::from_builtin();
    let ok = match &ms {
        Ok(t) => {
            let mut good = t.len() == 5 && t.last_used() == DOMAIN_CHECKPOINT;
            let mut k = 1usize;
            while k < t.len() {
                let a = t.at(k - 1);
                let b = t.at(k);
                match (a, b) {
                    (Some(x), Some(y)) => {
                        if y.used <= x.used || y.used - x.used != 6_000 {
                            good = false;
                        }
                    }
                    _ => good = false,
                }
                k += 1;
            }
            good
        }
        Err(_) => false,
    };
    let det = format!("len={} last={}", ms.as_ref().map(|t| t.len()).unwrap_or(0), ms.as_ref().map(|t| t.last_used()).unwrap_or(0));
    s.add_dyn("50100-J1-里程碑等差", ok, &det);

    let mut led = full_ledger();

    // J1-4 装载：20 条、全未冻结、账面行数为 0。
    let ok = led.len() == BATCH_SIZE && led.frozen_count() == 0 && led.lines_sum() == 0;
    s.add("50100-J1-装载", ok, if ok { "" } else { "装载后状态不对" });

    // J1-5 冻结 20/20：逐条恰好配额冻结。
    let frozen = led.freeze_all();
    let ok = frozen == Ok(BATCH_SIZE as u32)
        && led.frozen_count() == BATCH_SIZE as u32
        && led.lines_sum() == 6_000;
    let det = format!("frozen={:?} lines={}", frozen, led.lines_sum());
    s.add_dyn("50100-J1-二十条冻结", ok, &det);

    // J1-6 域账 150,000/240,000（625‰）。
    let acct = led.domain();
    let ok = acct.used == DOMAIN_CHECKPOINT && acct.budget == DOMAIN_BUDGET && acct.permille == 625;
    let det = format!("{}/{} {}permille", acct.used, acct.budget, acct.permille);
    s.add_dyn("50100-J1-域账检查点", ok, &det);

    // J1-7 批印入册：印面 20/20、域账 150,000、里程碑对齐、摘要非零。
    let stamp = led.seal();
    let ok = match &stamp {
        Ok(st) => {
            st.count == 20
                && st.quota_sum == 6_000
                && st.lines_sum == 6_000
                && st.domain_used == 150_000
                && st.budget == 240_000
                && st.milestone_used == 150_000
                && st.digest != 0
        }
        Err(_) => false,
    };
    let det = match &stamp {
        Ok(st) => format!(
            "count={} used={}/{} ms={} dg={:08x}",
            st.count, st.domain_used, st.budget, st.milestone_used, st.digest
        ),
        Err(e) => format!("err={}", e.code()),
    };
    s.add_dyn("50100-J1-批印入册", ok, &det);

    // J1-8 印面审计通过（落印后立刻可审计）。
    let ok = led.audit().is_ok() && led.is_sealed() && led.stamp().is_some();
    s.add("50100-J1-印面审计", ok, if ok { "" } else { "落印后审计失败" });

    // J1-9 印后只读：冻结被拒、重复落印被拒。
    let imm = led.freeze(NUM_LAST, 300);
    let again = led.seal();
    let ok = imm.err() == Some(SealError::Immutable) && again.err() == Some(SealError::AlreadySealed);
    s.add("50100-J1-印后只读", ok, if ok { "" } else { "印后仍可写" });

    // J1-10 未全冻结：19 条冻结后落印报 NotFrozen。
    let mut partial = full_ledger();
    let mut k = 0usize;
    while k < BATCH_SIZE - 1 {
        if let Some(r) = PLAN.get(k) {
            let _ = partial.freeze(r.num, r.quota);
        }
        k += 1;
    }
    let ok = partial.frozen_count() == 19 && partial.seal().err() == Some(SealError::NotFrozen);
    s.add("50100-J1-未全冻结", ok, if ok { "" } else { "未全冻结仍落印" });

    // J1-11 域账多一行：150,025 ≠ 检查点 → Account。
    let mut off = full_ledger();
    let _ = off.freeze_all();
    if let Some(e) = off.entries.get_mut(0) {
        e.lines = e.lines.saturating_add(1);
    }
    let ok = off.domain().used == DOMAIN_CHECKPOINT + DOMAIN_FACTOR
        && off.seal().err() == Some(SealError::Account);
    let det = format!("used={}", off.domain().used);
    s.add_dyn("50100-J1-域账偏移", ok, &det);

    // J1-12 超预算：全按 8×配额 → 1,200,000 > 240,000 → Budget。
    let mut over = full_ledger();
    let mut m = 0usize;
    while m < over.len() {
        let pair = match over.entries.get(m) {
            Some(e) => (e.num, e.quota),
            None => break,
        };
        let _ = over.freeze(pair.0, pair.1.saturating_mul(QUOTA_SPREAD));
        m += 1;
    }
    let ok = over.domain().used > DOMAIN_BUDGET && over.seal().err() == Some(SealError::Budget);
    let det = format!("used={}", over.domain().used);
    s.add_dyn("50100-J1-超预算", ok, &det);

    // J1-13 账面行数上下界：低于配额 / 高于 8×配额 都拒，恰好边界放行。
    let mut bounds = full_ledger();
    let low = bounds.freeze(NUM_FIRST, 419);
    let high = bounds.freeze(NUM_FIRST, 420 * QUOTA_SPREAD + 1);
    let edge_lo = bounds.freeze(NUM_FIRST, 420);
    let mut bounds2 = full_ledger();
    let edge_hi = bounds2.freeze(NUM_FIRST, 420 * QUOTA_SPREAD);
    let ok = low.err() == Some(SealError::Quota)
        && high.err() == Some(SealError::Quota)
        && edge_lo.is_ok()
        && edge_hi.is_ok();
    s.add("50100-J1-行数边界", ok, if ok { "" } else { "账面行数边界失效" });

    // J1-14 印面确定性：两本同装订账的摘要与报告逐位一致。
    let mut a = full_ledger();
    let mut b = full_ledger();
    let _ = a.freeze_all();
    let _ = b.freeze_all();
    let sa = a.seal();
    let sb = b.seal();
    let ok = match (&sa, &sb) {
        (Ok(x), Ok(y)) => x == y && a.report() == b.report() && a.report().contains("B25 seal"),
        _ => false,
    };
    s.add("50100-J1-印面确定", ok, if ok { "" } else { "印面不确定" });

    // J1-15 印后篡改必抓：改一行后 audit → Digest。
    let mut t = full_ledger();
    let _ = t.freeze_all();
    let _ = t.seal();
    if let Some(e) = t.entries.get_mut(5) {
        e.lines = e.lines.saturating_add(2);
    }
    let ok = t.audit().err() == Some(SealError::Digest);
    s.add("50100-J1-篡改必抓", ok, if ok { "" } else { "印后篡改未被发现" });

    // 注入核：17 类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50100-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50100_checks_green() {
        let set = run_unx50100_checks();
        assert!(set.all_passed(), "UNX-F50100 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50100_plan_quota_is_6000() {
        let mut q = 0u32;
        let mut i = 0usize;
        while i < PLAN.len() {
            if let Some(r) = PLAN.get(i) {
                q += r.quota;
            }
            i += 1;
        }
        assert_eq!(q, 6_000);
        assert_eq!(q * DOMAIN_FACTOR, DOMAIN_CHECKPOINT);
    }

    #[test]
    fn unx50100_milestones_are_arithmetic() {
        let ms = MilestoneTable::from_builtin();
        let ok = match &ms {
            Ok(t) => t.len() == 5 && t.last_used() == 150_000 && !t.is_empty(),
            Err(_) => false,
        };
        assert!(ok, "builtin milestone table rejected");
    }
}
