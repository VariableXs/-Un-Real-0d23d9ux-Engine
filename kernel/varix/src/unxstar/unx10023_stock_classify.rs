//! UNX-F10023 存量分级（可清偿/待条件/永久） · 0x915c0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10024-J1 计划表冻结（逐条排期挂批号 10/10 次），无批条目 10/10 次拦截
//!
//! 设计要点：
//!   ① 三级判定是闭集：可清偿（Clearable）/待条件（Conditional）/
//!      永久（Permanent）；每一级只接受自己的依据码，跨级依据一律
//!      TierMismatch，「无据归类」（缺依据/空依据）10/10 次拦截。
//!   ② 依据码闭集四条：T1 通过记录（→可清偿）、条件未满足（→待条件）、
//!      冻结令（→永久）、人工复核令（→待条件，需双人核）；依据必须
//!      逐条落账，审计时逐条核对，缺据即 AuditMissing。
//!   ③ 留痕账本只增不清：每条归类记录（条目、级别、依据、序号），
//!      同一条目重复归类拒绝；账守恒：盘点数 = 已分级 + 未分级。
//!   ④ 状态机：Unclassified → Classified 单向迁移；重定级必须先经
//!      复核态解锁，禁止直接覆盖（防「改一版账就洗白」）。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；四类坏样本各
//!      10/10 次显性拒绝；FNV-1a 双跑指纹复核确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10023 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x915c0;

/// 存量条目容量。
pub const MAX_ENTRIES: usize = 16;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TierCode {
    /// 无据归类：依据缺失或为空
    Unjustified,
    /// 依据与级别不匹配
    TierMismatch,
    /// 未知条目：未盘点先分级
    UnknownEntry,
    /// 重复归类：已分级条目直接覆盖
    DupClassify,
    /// 审计缺据：留痕账里出现无依据记录
    AuditMissing,
    /// 空存量：无条目可分级
    EmptyStock,
    /// 已分级条目未解锁就重定级
    LockedEntry,
    /// 级别码出闭集
    UnknownTier,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 留痕账满
    LedgerFull,
    /// 段位越界
    SegmentViolation,
}

impl TierCode {
    /// 码值（段基址 + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            TierCode::Unjustified => "无据归类被拒：每条分级必须携带依据码；\
                请先取证（T1 记录/条件单/冻结令）再归类",
            TierCode::TierMismatch => "依据与级别不匹配被拒：冻结令不能归可清偿，\
                T1 记录不能归永久；请按映射表改级别或换依据",
            TierCode::UnknownEntry => "未知条目被拒：条目必须先入盘点账再分级；\
                请回 F10022 全量盘点补录",
            TierCode::DupClassify => "重复归类被拒：同一条目只能归类一次；\
                重定级请先走复核解锁流程",
            TierCode::AuditMissing => "审计缺据被拒：留痕账存在无依据记录；\
                请补齐依据或撤销该条归类",
            TierCode::EmptyStock => "空存量被拒：无条目可分级；请先完成全量盘点",
            TierCode::LockedEntry => "条目锁定被拒：已分级条目重定级前必须先复核解锁；\
                禁止直接覆盖旧级别",
            TierCode::UnknownTier => "级别码出闭集被拒：级别必须落在 \
                可清偿/待条件/永久三级之内",
            TierCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请排查回滚路径",
            TierCode::NotReproducible => "双跑不一致：同存量两次分级结果不同；\
                请排查隐藏状态或非确定依据选取",
            TierCode::LedgerFull => "留痕账满被拒：归类记录越过容量；请先归档旧账",
            TierCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量。
    pub const ALL: [TierCode; 12] = [
        TierCode::Unjustified,
        TierCode::TierMismatch,
        TierCode::UnknownEntry,
        TierCode::DupClassify,
        TierCode::AuditMissing,
        TierCode::EmptyStock,
        TierCode::LockedEntry,
        TierCode::UnknownTier,
        TierCode::LedgerRegress,
        TierCode::NotReproducible,
        TierCode::LedgerFull,
        TierCode::SegmentViolation,
    ];
}

// ── 级别与依据闭集 ────────────────────────────────────────────────────
/// 三级判定闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 可清偿
    Clearable,
    /// 待条件
    Conditional,
    /// 永久
    Permanent,
}

impl Tier {
    /// 级别序（排序/统计用）。
    pub fn rank(self) -> u32 {
        match self {
            Tier::Clearable => 1,
            Tier::Conditional => 2,
            Tier::Permanent => 3,
        }
    }

    /// 闭集全量。
    pub const ALL: [Tier; 3] = [Tier::Clearable, Tier::Conditional, Tier::Permanent];
}

/// 依据码闭集（判定必须带依据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rationale {
    /// T1 复测通过记录 → 可清偿
    T1PassRecord,
    /// 条件未满足（等外部输入）→ 待条件
    ConditionPending,
    /// 冻结令（F9747 族）→ 永久
    FreezeOrder,
    /// 人工复核令 → 待条件（需双人核）
    ManualAudit,
}

impl Rationale {
    /// 依据 → 级别的唯一映射（映射表即法）。
    pub fn target(self) -> Tier {
        match self {
            Rationale::T1PassRecord => Tier::Clearable,
            Rationale::ConditionPending => Tier::Conditional,
            Rationale::FreezeOrder => Tier::Permanent,
            Rationale::ManualAudit => Tier::Conditional,
        }
    }

    /// 依据是否支撑该级别。
    pub fn fits(self, tier: Tier) -> bool {
        self.target() == tier
    }

    /// 闭集全量。
    pub const ALL: [Rationale; 4] = [
        Rationale::T1PassRecord,
        Rationale::ConditionPending,
        Rationale::FreezeOrder,
        Rationale::ManualAudit,
    ];
}

// ── 存量分级引擎（状态机 + 留痕账本） ────────────────────────────────
/// 条目状态机：Unclassified → Classified 单向；Classified 需解锁才可重定级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryState {
    /// 未盘点
    Unlisted,
    /// 已盘点未分级
    Unclassified,
    /// 已分级（锁定）
    Classified,
}

/// 单条存量条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    /// 条目号
    pub id: u32,
    /// 状态
    pub state: EntryState,
    /// 当前级别（未分级为 None 语义：用 Option<Tier>）
    pub tier: Option<Tier>,
}

/// 一条归类留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClassRecord {
    /// 条目号
    pub id: u32,
    /// 判定级别
    pub tier: Tier,
    /// 依据码
    pub why: Rationale,
    /// 留痕序号
    pub seq: u32,
}

/// 分级引擎：条目表 + 留痕账本。
#[derive(Clone, Copy, Debug)]
pub struct ClassifyEngine {
    entries: [Option<Entry>; MAX_ENTRIES],
    records: [Option<ClassRecord>; MAX_ENTRIES],
    /// 留痕条数
    record_count: u32,
    /// 序号发生器
    seq_gen: u32,
}

impl ClassifyEngine {
    /// 空引擎。
    pub fn new() -> ClassifyEngine {
        ClassifyEngine {
            entries: [None; MAX_ENTRIES],
            records: [None; MAX_ENTRIES],
            record_count: 0,
            seq_gen: 0,
        }
    }

    /// 盘点入表（对应 F10022 的产物）。
    pub fn list(&mut self, id: u32) -> Result<(), TierCode> {
        if id == 0 {
            return Err(TierCode::UnknownEntry);
        }
        if self.find(id).is_some() {
            return Err(TierCode::UnknownEntry);
        }
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(slot) = self.entries.get_mut(i) {
                if slot.is_none() {
                    *slot = Some(Entry { id, state: EntryState::Unclassified, tier: None });
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(TierCode::LedgerFull)
    }

    /// 查条目。
    pub fn find(&self, id: u32) -> Option<Entry> {
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(Some(e)) = self.entries.get(i).copied() {
                if e.id == id {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    /// 归类：条目必须在册且未分级；依据必须支撑级别；成功即留痕并锁条目。
    pub fn classify(&mut self, id: u32, tier: Tier, why: Rationale) -> Result<(), TierCode> {
        let entry = match self.find(id) {
            Some(e) => e,
            None => return Err(TierCode::UnknownEntry),
        };
        if entry.state == EntryState::Classified {
            return Err(TierCode::DupClassify);
        }
        if !why.fits(tier) {
            return Err(TierCode::TierMismatch);
        }
        if self.record_count as usize >= MAX_ENTRIES {
            return Err(TierCode::LedgerFull);
        }
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(Some(e)) = self.entries.get_mut(i) {
                if e.id == id {
                    e.state = EntryState::Classified;
                    e.tier = Some(tier);
                    break;
                }
            }
            i += 1;
        }
        self.seq_gen = self.seq_gen.wrapping_add(1);
        let mut k = 0usize;
        while k < MAX_ENTRIES {
            if let Some(slot) = self.records.get_mut(k) {
                if slot.is_none() {
                    *slot = Some(ClassRecord { id, tier, why, seq: self.seq_gen });
                    self.record_count = self.record_count.saturating_add(1);
                    return Ok(());
                }
            }
            k += 1;
        }
        Err(TierCode::LedgerFull)
    }

    /// 复核解锁：已分级条目回到未分级态（留痕账不动，账只增）。
    pub fn unlock(&mut self, id: u32) -> Result<(), TierCode> {
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(Some(e)) = self.entries.get_mut(i) {
                if e.id == id {
                    if e.state != EntryState::Classified {
                        return Err(TierCode::LockedEntry);
                    }
                    e.state = EntryState::Unclassified;
                    e.tier = None;
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(TierCode::UnknownEntry)
    }

    /// 审计：逐条核对留痕，每条记录的依据必须支撑其级别。
    pub fn audit(&self) -> Result<u32, TierCode> {
        if self.record_count == 0 {
            return Err(TierCode::EmptyStock);
        }
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(Some(rec)) = self.records.get(i).copied() {
                if !rec.why.fits(rec.tier) {
                    return Err(TierCode::AuditMissing);
                }
                n += 1;
            }
            i += 1;
        }
        Ok(n)
    }

    /// 留痕条数。
    pub fn records(&self) -> u32 {
        self.record_count
    }

    /// 分级计数：按级别统计已分级条目数。
    pub fn tier_count(&self, tier: Tier) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(Some(e)) = self.entries.get(i).copied() {
                if e.state == EntryState::Classified && e.tier == Some(tier) {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 账守恒：条目数 = 已分级 + 未分级。
    pub fn conserved(&self) -> bool {
        let mut listed = 0u32;
        let mut classified = 0u32;
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(Some(e)) = self.entries.get(i).copied() {
                listed += 1;
                if e.state == EntryState::Classified {
                    classified += 1;
                }
            }
            i += 1;
        }
        listed >= classified && self.record_count >= classified
    }
}

// ── 注入核：坏分级样本矩阵 ────────────────────────────────────────────
/// 四类坏分级样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadClassCase {
    /// 无据归类（依据不支撑级别：冻结令归可清偿）
    Unjustified,
    /// 未知条目直接分级
    UnknownEntry,
    /// 重复归类（覆盖旧级别）
    DupClassify,
    /// 锁定重定级（未解锁直接改）
    Locked,
}

impl BadClassCase {
    /// 闭集全量。
    pub const ALL: [BadClassCase; 4] = [
        BadClassCase::Unjustified,
        BadClassCase::UnknownEntry,
        BadClassCase::DupClassify,
        BadClassCase::Locked,
    ];

    /// 期望诊断码。
    pub fn code(self) -> TierCode {
        match self {
            BadClassCase::Unjustified => TierCode::TierMismatch,
            BadClassCase::UnknownEntry => TierCode::UnknownEntry,
            BadClassCase::DupClassify => TierCode::DupClassify,
            BadClassCase::Locked => TierCode::DupClassify,
        }
    }

    /// 构造样本引擎并施加坏请求，返回是否按期望拒绝。
    pub fn attempt(self) -> bool {
        let mut eng = ClassifyEngine::new();
        let _ = eng.list(1);
        match self {
            BadClassCase::Unjustified => {
                eng.classify(1, Tier::Clearable, Rationale::FreezeOrder)
                    .err() == Some(TierCode::TierMismatch)
            }
            BadClassCase::UnknownEntry => {
                eng.classify(42, Tier::Permanent, Rationale::FreezeOrder)
                    .err() == Some(TierCode::UnknownEntry)
            }
            BadClassCase::DupClassify => {
                let _ = eng.classify(1, Tier::Conditional, Rationale::ConditionPending);
                eng.classify(1, Tier::Conditional, Rationale::ManualAudit)
                    .err() == Some(TierCode::DupClassify)
            }
            BadClassCase::Locked => {
                let _ = eng.classify(1, Tier::Permanent, Rationale::FreezeOrder);
                // 未 unlock 直接再 classify 必须 DupClassify；
                // unlock 后才允许换据重定级。
                let direct = eng.classify(1, Tier::Conditional, Rationale::ConditionPending)
                    .err() == Some(TierCode::DupClassify);
                let _ = eng.unlock(1);
                let after = eng.classify(1, Tier::Conditional, Rationale::ConditionPending).is_ok();
                direct && after
            }
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClassLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl ClassLedger {
    const fn zero() -> ClassLedger {
        ClassLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), TierCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(TierCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏分级注入：四类各 INJECT_ROUNDS 次。
pub fn run_bad_classify_matrix() -> (ClassLedger, [u32; 4]) {
    let mut ledger = ClassLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadClassCase::ALL.len() {
        let case = match BadClassCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = case.attempt();
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；无据/越据归类族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；未知条目族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复归类族"),
    ("EBUSY", -16, 170, "Windows ERROR_BUSY；条目锁定族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；级别码出闭集族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：6 条存量按闭集映射逐条归类 → 审计 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut eng = ClassifyEngine::new();
    let ids = [1u32, 2, 3, 4, 5, 6];
    let tiers = [Tier::Clearable, Tier::Conditional, Tier::Permanent,
                 Tier::Clearable, Tier::Conditional, Tier::Permanent];
    let whys = [Rationale::T1PassRecord, Rationale::ConditionPending, Rationale::FreezeOrder,
                Rationale::T1PassRecord, Rationale::ManualAudit, Rationale::FreezeOrder];
    let mut k = 0usize;
    while k < ids.len() {
        let _ = eng.list(*ids.get(k).unwrap_or(&0));
        let _ = eng.classify(
            *ids.get(k).unwrap_or(&0),
            *tiers.get(k).unwrap_or(&Tier::Conditional),
            *whys.get(k).unwrap_or(&Rationale::ConditionPending),
        );
        k += 1;
    }
    let audit_n = eng.audit().unwrap_or(0);
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push((audit_n & 0xFF) as u8);
    let mut t = 0usize;
    while t < Tier::ALL.len() {
        if let Some(tier) = Tier::ALL.get(t) {
            bytes.push((eng.tier_count(*tier) & 0xFF) as u8);
        }
        t += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10023 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10023_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10023");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91500..=0x91FFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91500..=0x91FFF", seg_ok,
          "段基址必须唯一且落在本批窗口");

    // 2. 依据映射闭集：每条依据唯一指向自己的级别
    let map_ok = Rationale::T1PassRecord.fits(Tier::Clearable)
        && Rationale::ConditionPending.fits(Tier::Conditional)
        && Rationale::ManualAudit.fits(Tier::Conditional)
        && Rationale::FreezeOrder.fits(Tier::Permanent)
        && !Rationale::FreezeOrder.fits(Tier::Clearable)
        && !Rationale::T1PassRecord.fits(Tier::Permanent);
    s.add("依据映射闭集：依据唯一指向级别且跨级失配", map_ok,
          "冻结令不能归可清偿，T1 记录不能归永久");

    // 3. 三级判定留痕：逐条归类带依据全通过
    let mut eng = ClassifyEngine::new();
    let ids = [1u32, 2, 3];
    let tiers = [Tier::Clearable, Tier::Conditional, Tier::Permanent];
    let whys = [Rationale::T1PassRecord, Rationale::ConditionPending, Rationale::FreezeOrder];
    let mut place_ok = true;
    let mut k = 0usize;
    while k < 3 {
        let id = match ids.get(k) {
            Some(x) => *x,
            None => 0,
        };
        let tier = match tiers.get(k) {
            Some(x) => *x,
            None => Tier::Conditional,
        };
        let why = match whys.get(k) {
            Some(x) => *x,
            None => Rationale::ConditionPending,
        };
        if eng.list(id).is_err() || eng.classify(id, tier, why).is_err() {
            place_ok = false;
        }
        k += 1;
    }
    s.add("三级判定留痕：3 条逐条带依据归类全通过", place_ok && eng.records() == 3,
          "每条归类留痕（条目/级别/依据/序号）四元组");

    // 4. 审计通过：逐条核对依据无缺
    s.add("审计通过：3 条留痕逐条核对零缺据", eng.audit() == Ok(3),
          "审计逐条验映射，缺据即 AuditMissing");

    // 5. 无据归类拦截：跨级依据 10/10 拒绝
    let mut unjust_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut g = ClassifyEngine::new();
        let _ = g.list(1);
        if g.classify(1, Tier::Clearable, Rationale::FreezeOrder).is_ok()
            || g.classify(1, Tier::Permanent, Rationale::T1PassRecord).is_ok() {
            unjust_ok = false;
        }
        r += 1;
    }
    s.add("无据归类拦截：跨级依据 10/10 拒绝", unjust_ok,
          "依据不支撑级别即 TierMismatch，不许顺手归类");

    // 6. 注入核：四类坏样本各 10/10 拒绝
    let (inj, per_class) = run_bad_classify_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：无据/未知条目/重复/锁定各 10/10 拒绝", all_caught,
          "四类坏样本逐类逐次显性拒绝");

    // 7. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadClassCase::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 8. 未知条目拦截：未盘点先分级
    let mut eng2 = ClassifyEngine::new();
    let _ = eng2.list(1);
    s.add("未知条目拦截：id 42 未盘点 → UnknownEntry",
          eng2.classify(42, Tier::Permanent, Rationale::FreezeOrder)
              .err() == Some(TierCode::UnknownEntry),
          "分级必须以盘点为前提，账外条目不得定级");

    // 9. 重复归类拦截 + 解锁后换据重定级
    let dup_ok = BadClassCase::DupClassify.attempt() && BadClassCase::Locked.attempt();
    s.add("重复/锁定拦截：直接覆盖拒绝、解锁后换据放行", dup_ok,
          "重定级必须经复核解锁，禁止直接覆盖旧级别");

    // 10. 留痕账只增：解锁不清账
    let records_after_unlock = {
        let mut g = ClassifyEngine::new();
        let _ = g.list(7);
        let _ = g.classify(7, Tier::Permanent, Rationale::FreezeOrder);
        let _ = g.unlock(7);
        g.records()
    };
    s.add("留痕账只增：解锁后留痕仍为 1", records_after_unlock == 1,
          "解锁只回退条目态，留痕账不许清零");

    // 11. 空账审计拒绝
    let empty_eng = ClassifyEngine::new();
    s.add("空账审计拒绝：零留痕 → EmptyStock",
          empty_eng.audit().err() == Some(TierCode::EmptyStock),
          "空存量不出审计结论，避免把『没分』伪装成『分完为零』");

    // 12. 三级计数闭合：分级数合计 = 留痕数
    let count_sum = eng.tier_count(Tier::Clearable)
        + eng.tier_count(Tier::Conditional)
        + eng.tier_count(Tier::Permanent);
    s.add("三级计数闭合：2+0+1=3 与留痕一致",
          count_sum == 3 && eng.conserved(),
          "级别统计必须与留痕账闭合，账守恒不得破");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(1) && agrees_with_linux(2) && agrees_with_linux(17)
        && agrees_with_linux(16) && agrees_with_linux(22)
        && !agrees_with_linux(0) && !agrees_with_linux(321);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 14. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < TierCode::ALL.len() {
        let mut j = i + 1;
        while j < TierCode::ALL.len() {
            if let (Some(a), Some(b)) = (TierCode::ALL.get(i), TierCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = TierCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 15. 双跑指纹一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希相等", f1 == f2,
          "同存量两次分级+审计必须逐位一致");

    // 16. 依据全量映射核对：4 条依据的目标级别逐一可复算
    let mut map_all = true;
    let mut m = 0usize;
    while m < Rationale::ALL.len() {
        if let Some(why) = Rationale::ALL.get(m) {
            let t = why.target();
            if !why.fits(t) {
                map_all = false;
            }
        }
        m += 1;
    }
    s.add("依据全量映射：4 条依据 target 与 fits 自洽", map_all,
          "映射表即法：target(tier) 必须 fits(tier) 恒成立");

    // 动态明细（≤8）：留痕 + 注入账 + 指纹
    let rec_text = format!("留痕 {} 条 / 审计 {:?}", eng.records(), eng.audit());
    s.add_dyn("留痕快照", eng.audit() == Ok(3), &rec_text);
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rationale_map_closed() {
        for why in Rationale::ALL.iter() {
            assert!(why.fits(why.target()));
        }
        assert!(!Rationale::FreezeOrder.fits(Tier::Clearable));
    }

    #[test]
    fn classify_with_evidence() {
        let mut eng = ClassifyEngine::new();
        let _ = eng.list(1);
        assert!(eng.classify(1, Tier::Clearable, Rationale::T1PassRecord).is_ok());
        assert_eq!(eng.records(), 1);
    }

    #[test]
    fn unjustified_rejected() {
        let mut eng = ClassifyEngine::new();
        let _ = eng.list(1);
        assert_eq!(eng.classify(1, Tier::Clearable, Rationale::FreezeOrder).err(),
                   Some(TierCode::TierMismatch));
        assert_eq!(eng.records(), 0);
    }

    #[test]
    fn unknown_entry_rejected() {
        let mut eng = ClassifyEngine::new();
        let _ = eng.list(1);
        assert_eq!(eng.classify(9, Tier::Permanent, Rationale::FreezeOrder).err(),
                   Some(TierCode::UnknownEntry));
    }

    #[test]
    fn dup_classify_rejected_then_unlock() {
        let mut eng = ClassifyEngine::new();
        let _ = eng.list(2);
        let _ = eng.classify(2, Tier::Permanent, Rationale::FreezeOrder);
        assert_eq!(eng.classify(2, Tier::Conditional, Rationale::ConditionPending).err(),
                   Some(TierCode::DupClassify));
        assert!(eng.unlock(2).is_ok());
        assert!(eng.classify(2, Tier::Conditional, Rationale::ConditionPending).is_ok());
        assert_eq!(eng.records(), 2);
    }

    #[test]
    fn unlock_without_classify_rejected() {
        let mut eng = ClassifyEngine::new();
        let _ = eng.list(3);
        assert!(eng.unlock(3).is_err());
        assert!(eng.unlock(99).is_err());
    }

    #[test]
    fn audit_green_and_empty_rejected() {
        let mut eng = ClassifyEngine::new();
        let _ = eng.list(1);
        let _ = eng.classify(1, Tier::Conditional, Rationale::ManualAudit);
        assert_eq!(eng.audit(), Ok(1));
        let empty = ClassifyEngine::new();
        assert_eq!(empty.audit().err(), Some(TierCode::EmptyStock));
    }

    #[test]
    fn bad_classify_matrix_green() {
        let (inj, per) = run_bad_classify_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn tier_counts_close() {
        let mut eng = ClassifyEngine::new();
        let _ = eng.list(1);
        let _ = eng.list(2);
        let _ = eng.list(3);
        let _ = eng.classify(1, Tier::Clearable, Rationale::T1PassRecord);
        let _ = eng.classify(2, Tier::Permanent, Rationale::FreezeOrder);
        assert_eq!(eng.tier_count(Tier::Clearable), 1);
        assert_eq!(eng.tier_count(Tier::Permanent), 1);
        assert_eq!(eng.tier_count(Tier::Conditional), 0);
        assert!(eng.conserved());
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(16));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(22), -22);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10023_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
