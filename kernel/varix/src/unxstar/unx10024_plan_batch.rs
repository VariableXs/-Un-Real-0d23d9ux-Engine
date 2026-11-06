//! UNX-F10024 清偿计划表（批次↔条目） · 0x91600段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10025-J1 逐条重测留痕（原判据号复测 10/10 次），代测 10/10 次拦截
//!
//! 设计要点：
//!   ① 计划表把「批次 ↔ 条目」双向挂死：每个排期条目必须挂唯一批号，
//!      冻结时逐条核对，无批条目 10/10 次拦截；批次一旦封板（Sealed）
//!      不得再挂新条目，防冻结后排期漂移。
//!   ② 只有可清偿条目可排期：待条件/永久条目试图入批一律 NotClearable
//!      （分级口径与 F10023 对齐），不许「先排了再说」。
//!   ③ 批次状态机：Open → Sealed 单向；封板后 schedule 拒绝；批次
//!      容量满显性拒绝，不许静默挤占。
//!   ④ 账守恒：在册可清偿条目数 = 已挂批数 + 未挂批数，逃逸恒为 0；
//!      冻结成功的前提是未挂批数归零。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸；四类坏样本（无批冻结/重复
//!      挂批/封板后挂批/超容挂批）各 10/10 次显性拒绝；FNV-1a 双跑
//!      指纹复核确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10024 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91600;

/// 批次容量上限。
pub const MAX_BATCHES: usize = 8;

/// 条目容量上限。
pub const MAX_ENTRIES: usize = 16;

/// 单批条目容量。
pub const BATCH_CAP: u32 = 8;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlanCode {
    /// 未知批次
    UnknownBatch,
    /// 未知条目
    UnknownEntry,
    /// 重复挂批
    DupSchedule,
    /// 批次已封板仍挂批
    BatchSealed,
    /// 批次超容
    BatchFull,
    /// 非可清偿条目入批
    NotClearable,
    /// 冻结时存在无批条目
    UnscheduledEntry,
    /// 零排期冻结
    NothingScheduled,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 表满
    TableFull,
    /// 段位越界
    SegmentViolation,
}

impl PlanCode {
    /// 码值（段基址 + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            PlanCode::UnknownBatch => "未知批次被拒：批次必须先开户再挂条目；\
                请先 create_batch 取得批号",
            PlanCode::UnknownEntry => "未知条目被拒：条目必须先入册再排期；\
                请回盘点/分级流程补录",
            PlanCode::DupSchedule => "重复挂批被拒：一条目只挂一个批次；\
                换批请先摘牌再重挂",
            PlanCode::BatchSealed => "封板后挂批被拒：批次已冻结不得新增排期；\
                请改挂未封板批次或走重开流程",
            PlanCode::BatchFull => "批次超容被拒：单批条目数越过 BATCH_CAP；\
                请开新批次分摊",
            PlanCode::NotClearable => "非可清偿条目入批被拒：只有可清偿级可排期；\
                待条件/永久条目请按 F10023 口径处理",
            PlanCode::UnscheduledEntry => "无批条目被拒：冻结时存在未挂批的可清偿条目；\
                请逐条补批号或显式移出本期",
            PlanCode::NothingScheduled => "零排期冻结被拒：没有任何挂批条目时不得冻结计划表；\
                请先完成排期",
            PlanCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请排查回滚路径",
            PlanCode::NotReproducible => "双跑不一致：同计划表两次冻结结果不同；\
                请排查隐藏状态或非确定遍历",
            PlanCode::TableFull => "表满被拒：批次/条目表越过容量；请先归档旧表",
            PlanCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量。
    pub const ALL: [PlanCode; 12] = [
        PlanCode::UnknownBatch,
        PlanCode::UnknownEntry,
        PlanCode::DupSchedule,
        PlanCode::BatchSealed,
        PlanCode::BatchFull,
        PlanCode::NotClearable,
        PlanCode::UnscheduledEntry,
        PlanCode::NothingScheduled,
        PlanCode::LedgerRegress,
        PlanCode::NotReproducible,
        PlanCode::TableFull,
        PlanCode::SegmentViolation,
    ];
}

// ── 条目级别（与 F10023 对齐） ────────────────────────────────────────
/// 计划条目级别闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlanKind {
    /// 可清偿（可排期）
    Clearable,
    /// 待条件（不可排期）
    Conditional,
    /// 永久（不可排期）
    Permanent,
}

impl PlanKind {
    /// 是否允许排期。
    pub fn schedulable(self) -> bool {
        matches!(self, PlanKind::Clearable)
    }

    /// 闭集全量。
    pub const ALL: [PlanKind; 3] = [PlanKind::Clearable, PlanKind::Conditional, PlanKind::Permanent];
}

// ── 批次状态机 ────────────────────────────────────────────────────────
/// 批次状态：Open → Sealed 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchState {
    /// 开放（可挂批）
    Open,
    /// 封板（冻结）
    Sealed,
}

/// 单个批次。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Batch {
    /// 批号
    pub id: u32,
    /// 状态
    pub state: BatchState,
    /// 已挂条目数
    pub count: u32,
    /// 开户序
    pub seq: u32,
}

/// 计划条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PlanEntry {
    /// 条目号
    pub id: u32,
    /// 级别
    pub kind: PlanKind,
    /// 挂批号（None = 未挂批）
    pub batch: Option<u32>,
}

// ── 计划表（注册表） ──────────────────────────────────────────────────
/// 清偿计划表：批次表 + 条目表 + 冻结账。
#[derive(Clone, Copy, Debug)]
pub struct PlanTable {
    batches: [Option<Batch>; MAX_BATCHES],
    entries: [Option<PlanEntry>; MAX_ENTRIES],
    /// 批次开户序
    batch_seq: u32,
    /// 累计挂批次数（只增）
    schedule_ops: u32,
    /// 已冻结批次数（冻结成功后记）
    frozen_batches: u32,
}

impl PlanTable {
    /// 空计划表。
    pub fn new() -> PlanTable {
        PlanTable {
            batches: [None; MAX_BATCHES],
            entries: [None; MAX_ENTRIES],
            batch_seq: 0,
            schedule_ops: 0,
            frozen_batches: 0,
        }
    }

    /// 开批次：批号从 1 起，表满拒绝。
    pub fn create_batch(&mut self) -> Result<u32, PlanCode> {
        let mut i = 0usize;
        while i < MAX_BATCHES {
            if self.batches[i].is_none() {
                self.batch_seq = self.batch_seq.wrapping_add(1);
                let id = self.batch_seq;
                self.batches[i] = Some(Batch { id, state: BatchState::Open, count: 0, seq: self.batch_seq });
                return Ok(id);
            }
            i += 1;
        }
        Err(PlanCode::TableFull)
    }

    /// 查批次。
    pub fn batch(&self, id: u32) -> Option<Batch> {
        let mut i = 0usize;
        while i < MAX_BATCHES {
            if let Some(Some(b)) = self.batches.get(i).copied() {
                if b.id == id {
                    return Some(b);
                }
            }
            i += 1;
        }
        None
    }

    /// 条目入册。
    pub fn list_entry(&mut self, id: u32, kind: PlanKind) -> Result<(), PlanCode> {
        if id == 0 {
            return Err(PlanCode::UnknownEntry);
        }
        if self.entry(id).is_some() {
            return Err(PlanCode::UnknownEntry);
        }
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(slot) = self.entries.get_mut(i) {
                if slot.is_none() {
                    *slot = Some(PlanEntry { id, kind, batch: None });
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(PlanCode::TableFull)
    }

    /// 查条目。
    pub fn entry(&self, id: u32) -> Option<PlanEntry> {
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

    /// 挂批：条目可清偿、批次存在且开放、条目未挂批、批次未满。
    pub fn schedule(&mut self, entry_id: u32, batch_id: u32) -> Result<(), PlanCode> {
        let entry = match self.entry(entry_id) {
            Some(e) => e,
            None => return Err(PlanCode::UnknownEntry),
        };
        if !entry.kind.schedulable() {
            return Err(PlanCode::NotClearable);
        }
        if entry.batch.is_some() {
            return Err(PlanCode::DupSchedule);
        }
        let b = match self.batch(batch_id) {
            Some(b) => b,
            None => return Err(PlanCode::UnknownBatch),
        };
        if b.state == BatchState::Sealed {
            return Err(PlanCode::BatchSealed);
        }
        if b.count >= BATCH_CAP {
            return Err(PlanCode::BatchFull);
        }
        let mut i = 0usize;
        while i < MAX_BATCHES {
            if let Some(Some(bk)) = self.batches.get_mut(i) {
                if bk.id == batch_id {
                    bk.count = bk.count.saturating_add(1);
                    break;
                }
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < MAX_ENTRIES {
            if let Some(Some(e)) = self.entries.get_mut(k) {
                if e.id == entry_id {
                    e.batch = Some(batch_id);
                    break;
                }
            }
            k += 1;
        }
        self.schedule_ops = self.schedule_ops.saturating_add(1);
        Ok(())
    }

    /// 未挂批的可清偿条目数。
    pub fn unscheduled(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(Some(e)) = self.entries.get(i).copied() {
                if e.kind.schedulable() && e.batch.is_none() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 已挂批的可清偿条目数。
    pub fn scheduled(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(Some(e)) = self.entries.get(i).copied() {
                if e.kind.schedulable() && e.batch.is_some() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 冻结计划表：存在无批条目即拒；零排期即拒；成功封板全部批次。
    /// 返回封板批次数。
    pub fn freeze(&mut self) -> Result<u32, PlanCode> {
        if self.unscheduled() > 0 {
            return Err(PlanCode::UnscheduledEntry);
        }
        if self.scheduled() == 0 {
            return Err(PlanCode::NothingScheduled);
        }
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MAX_BATCHES {
            if let Some(Some(b)) = self.batches.get_mut(i) {
                if b.state == BatchState::Open {
                    b.state = BatchState::Sealed;
                    n += 1;
                }
            }
            i += 1;
        }
        self.frozen_batches = self.frozen_batches.saturating_add(n);
        Ok(n)
    }

    /// 累计挂批操作数。
    pub fn ops(&self) -> u32 {
        self.schedule_ops
    }

    /// 已冻结批次数。
    pub fn frozen(&self) -> u32 {
        self.frozen_batches
    }

    /// 账守恒：可清偿在册 = 已挂批 + 未挂批。
    pub fn conserved(&self) -> bool {
        let mut clearable = 0u32;
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(Some(e)) = self.entries.get(i).copied() {
                if e.kind.schedulable() {
                    clearable += 1;
                }
            }
            i += 1;
        }
        self.scheduled().saturating_add(self.unscheduled()) == clearable
    }
}

// ── 注入核：坏排期样本矩阵 ────────────────────────────────────────────
/// 四类坏排期样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadPlanCase {
    /// 无批条目强行冻结
    FreezeWithGap,
    /// 同条目重复挂批
    DupSchedule,
    /// 封板批次再挂批
    SealedSchedule,
    /// 超容挂批
    OverCap,
}

impl BadPlanCase {
    /// 闭集全量。
    pub const ALL: [BadPlanCase; 4] = [
        BadPlanCase::FreezeWithGap,
        BadPlanCase::DupSchedule,
        BadPlanCase::SealedSchedule,
        BadPlanCase::OverCap,
    ];

    /// 期望诊断码。
    pub fn code(self) -> PlanCode {
        match self {
            BadPlanCase::FreezeWithGap => PlanCode::UnscheduledEntry,
            BadPlanCase::DupSchedule => PlanCode::DupSchedule,
            BadPlanCase::SealedSchedule => PlanCode::BatchSealed,
            BadPlanCase::OverCap => PlanCode::BatchFull,
        }
    }

    /// 构造样本表并施加坏请求，返回是否按期望拒绝。
    pub fn attempt(self) -> bool {
        let mut t = PlanTable::new();
        match self {
            BadPlanCase::FreezeWithGap => {
                let _ = t.create_batch();
                let _ = t.list_entry(1, PlanKind::Clearable);
                let _ = t.list_entry(2, PlanKind::Clearable);
                let _ = t.schedule(1, 1);
                t.freeze().err() == Some(PlanCode::UnscheduledEntry)
            }
            BadPlanCase::DupSchedule => {
                let _ = t.create_batch();
                let _ = t.list_entry(1, PlanKind::Clearable);
                let _ = t.schedule(1, 1);
                t.schedule(1, 1).err() == Some(PlanCode::DupSchedule)
            }
            BadPlanCase::SealedSchedule => {
                let _ = t.create_batch();
                // 先把在册条目全部挂满，冻结才能成功封板
                let _ = t.list_entry(1, PlanKind::Clearable);
                let _ = t.list_entry(2, PlanKind::Clearable);
                let _ = t.list_entry(3, PlanKind::Clearable);
                let _ = t.schedule(1, 1);
                let _ = t.schedule(2, 1);
                let _ = t.schedule(3, 1);
                let _ = t.freeze();
                // 封板后新条目挂批必须 BatchSealed
                let _ = t.list_entry(4, PlanKind::Clearable);
                t.schedule(4, 1).err() == Some(PlanCode::BatchSealed)
            }
            BadPlanCase::OverCap => {
                let _ = t.create_batch();
                let mut n = 1u32;
                while n <= BATCH_CAP {
                    let _ = t.list_entry(n, PlanKind::Clearable);
                    let _ = t.schedule(n, 1);
                    n += 1;
                }
                let _ = t.list_entry(BATCH_CAP + 1, PlanKind::Clearable);
                t.schedule(BATCH_CAP + 1, 1).err() == Some(PlanCode::BatchFull)
            }
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PlanLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl PlanLedger {
    const fn zero() -> PlanLedger {
        PlanLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), PlanCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(PlanCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏排期注入：四类各 INJECT_ROUNDS 次。
pub fn run_bad_plan_matrix() -> (PlanLedger, [u32; 4]) {
    let mut ledger = PlanLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadPlanCase::ALL.len() {
        let case = match BadPlanCase::ALL.get(ci) {
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
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；未知批次/条目族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复挂批族"),
    ("EBUSY", -16, 170, "Windows ERROR_BUSY；批次封板族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；批次超容族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；非可清偿入批/无批冻结族"),
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

/// 规范语料双跑指纹：2 批 6 条全挂批 → 冻结 → 摘哈希（批号×条目×冻结数）。
pub fn corpus_fingerprint() -> u32 {
    let mut t = PlanTable::new();
    let _ = t.create_batch();
    let _ = t.create_batch();
    let mut n = 1u32;
    while n <= 6 {
        let _ = t.list_entry(n, PlanKind::Clearable);
        let batch = if n <= 3 { 1u32 } else { 2u32 };
        let _ = t.schedule(n, batch);
        n += 1;
    }
    let frozen = t.freeze().unwrap_or(0);
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push((frozen & 0xFF) as u8);
    bytes.push((t.scheduled() & 0xFF) as u8);
    let mut k = 1u32;
    while k <= 6 {
        let b = match t.entry(k) {
            Some(e) => match e.batch {
                Some(b) => b,
                None => 0,
            },
            None => 0,
        };
        bytes.push((b & 0xFF) as u8);
        k += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10024 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10024_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10024");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91500..=0x91FFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91500..=0x91FFF", seg_ok,
          "段基址必须唯一且落在本批窗口");

    // 2. 排期资格闭集：只有 Clearable 可排期
    let kind_ok = PlanKind::Clearable.schedulable()
        && !PlanKind::Conditional.schedulable()
        && !PlanKind::Permanent.schedulable();
    s.add("排期资格：仅可清偿条目可入批", kind_ok,
          "待条件/永久条目不得进清偿批次");

    // 3. 正常流：2 批 4 条全挂批
    let mut t = PlanTable::new();
    let b1 = t.create_batch().unwrap_or(0);
    let b2 = t.create_batch().unwrap_or(0);
    let mut n = 1u32;
    let mut sched_ok = true;
    while n <= 4 {
        let _ = t.list_entry(n, PlanKind::Clearable);
        let batch = if n <= 2 { b1 } else { b2 };
        if t.schedule(n, batch).is_err() {
            sched_ok = false;
        }
        n += 1;
    }
    s.add("正常流：2 批 4 条全挂批且账守恒",
          sched_ok && t.scheduled() == 4 && t.unscheduled() == 0 && t.conserved(),
          "每条排期条目挂唯一批号，账守恒闭合");

    // 4. 挂批留痕：条目侧与批次侧双向一致
    let e1 = t.entry(1).unwrap_or(PlanEntry { id: 0, kind: PlanKind::Conditional, batch: None });
    let b1v = t.batch(b1).unwrap_or(Batch { id: 0, state: BatchState::Open, count: 0, seq: 0 });
    let b2v = t.batch(b2).unwrap_or(Batch { id: 0, state: BatchState::Open, count: 0, seq: 0 });
    s.add("挂批留痕：条目批号与批次计数双向一致",
          e1.batch == Some(b1) && b1v.count == 2 && b2v.count == 2 && t.ops() == 4,
          "批次计数 = 挂到该批的条目数，操作账逐笔累计");

    // 5. 注入核：四类坏样本各 10/10 拒绝
    let (inj, per_class) = run_bad_plan_matrix();
    let all_caught = per_class.iter().all(|x| *x == INJECT_ROUNDS);
    s.add("注入核：无批冻结/重复挂批/封板挂批/超容各 10/10 拒绝", all_caught,
          "四类坏样本逐类逐次显性拒绝");

    // 6. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadPlanCase::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 7. 无批条目拦截：冻结时缺批号立即点名
    let mut gap = PlanTable::new();
    let _ = gap.create_batch();
    let _ = gap.list_entry(1, PlanKind::Clearable);
    let _ = gap.list_entry(2, PlanKind::Clearable);
    let _ = gap.schedule(1, 1);
    s.add("无批条目拦截：冻结遇未挂批条目 → UnscheduledEntry",
          gap.freeze().err() == Some(PlanCode::UnscheduledEntry) && gap.unscheduled() == 1,
          "计划表冻结必须逐条核对批号，缺一即拒");

    // 8. 非可清偿拦截：待条件/永久入批拒绝
    let mut nc = PlanTable::new();
    let _ = nc.create_batch();
    let _ = nc.list_entry(1, PlanKind::Conditional);
    let _ = nc.list_entry(2, PlanKind::Permanent);
    let nc_ok = nc.schedule(1, 1).err() == Some(PlanCode::NotClearable)
        && nc.schedule(2, 1).err() == Some(PlanCode::NotClearable);
    s.add("非可清偿拦截：待条件/永久条目入批拒绝", nc_ok,
          "排期资格以分级口径为准，不得越级入批");

    // 9. 封板单向：冻结后再挂批必须拒绝
    let mut seal = PlanTable::new();
    let _ = seal.create_batch();
    let _ = seal.list_entry(1, PlanKind::Clearable);
    let _ = seal.schedule(1, 1);
    let frozen_n = seal.freeze().unwrap_or(0);
    let _ = seal.list_entry(2, PlanKind::Clearable);
    let seal_ok = frozen_n == 1
        && seal.batch(1).map(|b| b.state) == Some(BatchState::Sealed)
        && seal.schedule(2, 1).err() == Some(PlanCode::BatchSealed);
    s.add("封板单向：Open→Sealed 后挂批拒绝且批次不可逆", seal_ok,
          "冻结后批次状态单向迁移，排期漂移即拒绝");

    // 10. 零排期冻结拒绝
    let mut zero = PlanTable::new();
    let _ = zero.create_batch();
    s.add("零排期冻结拒绝：无挂批条目 → NothingScheduled",
          zero.freeze().err() == Some(PlanCode::NothingScheduled),
          "空计划表不得冻结，避免把『没排』伪装成『排完』");

    // 11. 批次超容：BATCH_CAP 满后第 9 条拒绝且账不变
    let cap_ok = BadPlanCase::OverCap.attempt();
    s.add("批次超容：第 9 条挂批拒绝", cap_ok,
          "单批容量显性上限，超容必须 BatchFull");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(2) && agrees_with_linux(17) && agrees_with_linux(16)
        && agrees_with_linux(28) && agrees_with_linux(1)
        && !agrees_with_linux(0) && !agrees_with_linux(5555);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < PlanCode::ALL.len() {
        let mut j = i + 1;
        while j < PlanCode::ALL.len() {
            if let (Some(a), Some(b)) = (PlanCode::ALL.get(i), PlanCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = PlanCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 14. 冻结账只增：二次冻结幂等（已封板批次不重复计数）
    let frozen2 = seal.freeze().unwrap_or(0);
    s.add("冻结幂等：二次冻结新增 0 批", frozen2 == 0 && seal.frozen() == 1,
          "Sealed 批次不再迁移，冻结账只增不重复");

    // 15. 双跑指纹一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希相等", f1 == f2,
          "同计划表两次冻结+摘哈希必须逐位一致");

    // 16. 批次表满：第 9 个批次拒绝
    let mut full = PlanTable::new();
    let mut got_full = false;
    let mut k = 0usize;
    while k < MAX_BATCHES + 2 {
        if full.create_batch().is_err() {
            got_full = true;
        }
        k += 1;
    }
    s.add("批次表满：越过 8 批后显性拒绝", got_full,
          "批次表容量显性上限，不许静默挤占");

    // 动态明细（≤8）：排期账 + 注入账 + 指纹
    let sched_text = format!(
        "挂批 {} / 未挂 {} / 冻结 {}",
        t.scheduled(), t.unscheduled(), t.frozen()
    );
    s.add_dyn("排期账快照", t.conserved(), &sched_text);
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
    fn create_and_find_batch() {
        let mut t = PlanTable::new();
        let b = t.create_batch().unwrap_or(0);
        assert_eq!(b, 1);
        assert!(t.batch(b).is_some());
        assert!(t.batch(99).is_none());
    }

    #[test]
    fn schedule_roundtrip() {
        let mut t = PlanTable::new();
        let b = t.create_batch().unwrap_or(0);
        let _ = t.list_entry(1, PlanKind::Clearable);
        assert!(t.schedule(1, b).is_ok());
        assert_eq!(t.entry(1).unwrap_or(PlanEntry { id: 0, kind: PlanKind::Conditional, batch: None }).batch, Some(b));
        assert_eq!(t.scheduled(), 1);
        assert_eq!(t.unscheduled(), 0);
    }

    #[test]
    fn dup_schedule_rejected() {
        let mut t = PlanTable::new();
        let b = t.create_batch().unwrap_or(0);
        let _ = t.list_entry(1, PlanKind::Clearable);
        let _ = t.schedule(1, b);
        assert_eq!(t.schedule(1, b).err(), Some(PlanCode::DupSchedule));
    }

    #[test]
    fn not_clearable_rejected() {
        let mut t = PlanTable::new();
        let b = t.create_batch().unwrap_or(0);
        let _ = t.list_entry(1, PlanKind::Permanent);
        assert_eq!(t.schedule(1, b).err(), Some(PlanCode::NotClearable));
    }

    #[test]
    fn sealed_batch_rejects_schedule() {
        let mut t = PlanTable::new();
        let b = t.create_batch().unwrap_or(0);
        let _ = t.list_entry(1, PlanKind::Clearable);
        let _ = t.schedule(1, b);
        assert!(t.freeze().is_ok());
        let _ = t.list_entry(2, PlanKind::Clearable);
        assert_eq!(t.schedule(2, b).err(), Some(PlanCode::BatchSealed));
    }

    #[test]
    fn freeze_with_gap_rejected() {
        let mut t = PlanTable::new();
        let _ = t.create_batch();
        let _ = t.list_entry(1, PlanKind::Clearable);
        assert_eq!(t.freeze().err(), Some(PlanCode::UnscheduledEntry));
    }

    #[test]
    fn freeze_nothing_rejected() {
        let mut t = PlanTable::new();
        let _ = t.create_batch();
        assert_eq!(t.freeze().err(), Some(PlanCode::NothingScheduled));
    }

    #[test]
    fn over_cap_rejected() {
        let mut t = PlanTable::new();
        let _ = t.create_batch();
        let mut n = 1u32;
        while n <= BATCH_CAP {
            let _ = t.list_entry(n, PlanKind::Clearable);
            let _ = t.schedule(n, 1);
            n += 1;
        }
        let _ = t.list_entry(BATCH_CAP + 1, PlanKind::Clearable);
        assert_eq!(t.schedule(BATCH_CAP + 1, 1).err(), Some(PlanCode::BatchFull));
    }

    #[test]
    fn bad_plan_matrix_green() {
        let (inj, per) = run_bad_plan_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(16));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(28), -28);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10024_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
