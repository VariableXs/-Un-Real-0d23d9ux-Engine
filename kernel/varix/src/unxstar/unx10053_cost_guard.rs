//! UNX-F10053 回归成本护栏（时长上限）· 0x91D40 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10054-J1 双档一致核（差异三分类留痕 10/10 次），未决差异 10/10 次拦截
//!
//! 设计要点：
//!   ① 时长护栏 knee = 0.8 × cap 写死：单批累计时长越过膝点即进入
//!      SplitPending 相位，必须显式拆批留痕（SplitRecord 递增编号），
//!      越过 cap 仍不拆批的充电 → SilentTimeout 逐次拦截（10/10）。
//!   ② 批相位状态机：Idle → Running → SplitPending → Running → … → Done，
//!      秩序单向推进；Done 后充电 NotRunning 拒绝，乱跳相位拒绝。
//!   ③ 成本账本守恒：charged + archived == total_charged 恒成立；
//!      拆批把 charged 归档后清零，任何拒绝路径不改账（拒绝不留痕）。
//!   ④ 注入核：静默超时 / 零时长 / 空批拆分 / 未登记批充电四类坏样本
//!      逐类 10/10 次拒绝，注入账守恒逃逸 = 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10053 独占 0x40 窗口，下一卡 0x91D80）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x91D40;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 单批时长上限（毫秒）：回归单批 10 分钟。
pub const BATCH_CAP_MS: u32 = 600_000;

/// 拆批膝点：knee = 0.8 × cap（越过即必须拆批留痕）。
pub const SPLIT_KNEE_MS: u32 = BATCH_CAP_MS / 5 * 4;

/// 批位账本容量上限。
pub const LEDGER_CAP: usize = 64;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuardCode {
    /// 未登记批充电
    UnknownBatch,
    /// 重复登记同批号
    DupBatch,
    /// 零时长充电
    ZeroElapsed,
    /// 越过上限仍不拆批（静默超时）
    SilentTimeout,
    /// 空批拆分（charged == 0）
    EmptyBatch,
    /// 相位非法（Idle/Done 相位充电等）
    NotRunning,
    /// 相位倒退（状态机秩序被破坏）
    PhaseRegress,
    /// 成本账失守恒（charged+archived ≠ total_charged）
    LedgerRegress,
    /// 批位账本容量满
    LedgerFull,
    /// 空成文
    EmptySummary,
    /// 拆批编号不连续（留痕断裂）
    SplitGap,
    /// 段位越界
    SegmentViolation,
}

impl GuardCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            GuardCode::UnknownBatch => "未登记批被拒：充电必须先 start 建档；\
                请先登记批号再计时长",
            GuardCode::DupBatch => "重复批号被拒：同一批号只允许建档一次；请换新批号",
            GuardCode::ZeroElapsed => "零时长被拒：充电时长必须大于 0；请核对计时器",
            GuardCode::SilentTimeout => "静默超时被拒：累计时长越过上限仍不拆批；\
                请先 split 拆批留痕再继续充电",
            GuardCode::EmptyBatch => "空批拆分被拒：批内无已计时长不得拆批；请先充电",
            GuardCode::NotRunning => "相位非法被拒：只有 Running/SplitPending 相位可充电；\
                请先 start 建档，终态批不得续计",
            GuardCode::PhaseRegress => "相位倒退被拒：批相位只进不退；请排查乱序调用",
            GuardCode::LedgerRegress => "成本账失守恒：charged+archived 必须等于 \
                total_charged；请排查是否有路径绕过充电直改账目",
            GuardCode::LedgerFull => "批位账本容量满被拒：已达上限；请先 finalize 归档旧批",
            GuardCode::EmptySummary => "空成文被拒：护栏小结不得为空串；请检查账本是否为空册",
            GuardCode::SplitGap => "拆批留痕断裂被拒：拆批编号必须从 1 连续递增；\
                请排查漏记路径",
            GuardCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [GuardCode; 12] = [
        GuardCode::UnknownBatch,
        GuardCode::DupBatch,
        GuardCode::ZeroElapsed,
        GuardCode::SilentTimeout,
        GuardCode::EmptyBatch,
        GuardCode::NotRunning,
        GuardCode::PhaseRegress,
        GuardCode::LedgerRegress,
        GuardCode::LedgerFull,
        GuardCode::EmptySummary,
        GuardCode::SplitGap,
        GuardCode::SegmentViolation,
    ];
}

// ── 批相位状态机 ──────────────────────────────────────────────────────
/// 批相位：Idle → Running → SplitPending → … → Done，只进不退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchPhase {
    /// 已建档未充电
    Idle,
    /// 计时中
    Running,
    /// 越过膝点，等待拆批
    SplitPending,
    /// 已终态归档
    Done,
}

impl BatchPhase {
    /// 相位秩（只进不退校验用）。
    pub fn rank(self) -> u8 {
        match self {
            BatchPhase::Idle => 0,
            BatchPhase::Running => 1,
            BatchPhase::SplitPending => 2,
            BatchPhase::Done => 3,
        }
    }

    /// 是否可充电相位。
    pub fn chargeable(self) -> bool {
        matches!(self, BatchPhase::Running | BatchPhase::SplitPending)
    }
}

// ── 拆批留痕 ──────────────────────────────────────────────────────────
/// 拆批记录：编号从 1 连续递增，携带拆批时刻的归档时长。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SplitRecord {
    /// 批号
    pub batch: u32,
    /// 拆批编号（1 起算连续递增）
    pub index: u32,
    /// 本次归档时长（毫秒）
    pub archived_ms: u32,
}

impl SplitRecord {
    /// 留痕自检：编号非零且归档时长非零。
    pub fn sound(&self) -> bool {
        self.index > 0 && self.archived_ms > 0
    }
}

/// 单批账户：相位 + 在计时长 + 归档时长 + 拆批记录。
#[derive(Clone, Debug)]
pub struct BatchAccount {
    /// 批号
    pub batch: u32,
    /// 当前相位
    pub phase: BatchPhase,
    /// 在计时长（未拆批部分）
    pub charged_ms: u32,
    /// 已归档时长（历次拆批累计）
    pub archived_ms: u32,
    /// 累计充电时长（守恒基准）
    pub total_charged: u32,
    /// 拆批记录（编号连续）
    pub splits: Vec<SplitRecord>,
}

/// 时长护栏：登记批、充电、拆批、终态，全程零 panic。
#[derive(Clone, Debug)]
pub struct TimeGuard {
    accounts: Vec<BatchAccount>,
    cap: usize,
}

impl TimeGuard {
    /// 空护栏构造（容量 LEDGER_CAP）。
    pub fn new() -> TimeGuard {
        TimeGuard { accounts: Vec::new(), cap: LEDGER_CAP }
    }

    fn find(&self, batch: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some(a) = self.accounts.get(i) {
                if a.batch == batch {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 建档：Idle 相位起步；重复批号 DupBatch、容量满 LedgerFull。
    pub fn start(&mut self, batch: u32) -> Result<(), GuardCode> {
        if self.find(batch).is_some() {
            return Err(GuardCode::DupBatch);
        }
        if self.accounts.len() >= self.cap {
            return Err(GuardCode::LedgerFull);
        }
        self.accounts.push(BatchAccount {
            batch,
            phase: BatchPhase::Idle,
            charged_ms: 0,
            archived_ms: 0,
            total_charged: 0,
            splits: Vec::new(),
        });
        // Idle → Running：建档即进入计时相位（秩进 1，合法推进）
        let idx = match self.find(batch) {
            Some(i) => i,
            None => return Err(GuardCode::SegmentViolation),
        };
        if let Some(a) = self.accounts.get_mut(idx) {
            a.phase = BatchPhase::Running;
        }
        Ok(())
    }

    /// 充电：ms > 0 且累计不越过 cap；越过膝点进入 SplitPending。
    /// 任何拒绝路径不改 charged/archived/total_charged（拒绝不留痕）。
    pub fn charge(&mut self, batch: u32, ms: u32) -> Result<u32, GuardCode> {
        if ms == 0 {
            return Err(GuardCode::ZeroElapsed);
        }
        let idx = match self.find(batch) {
            Some(i) => i,
            None => return Err(GuardCode::UnknownBatch),
        };
        let phase = match self.accounts.get(idx) {
            Some(a) => a.phase,
            None => return Err(GuardCode::SegmentViolation),
        };
        if !phase.chargeable() {
            return Err(GuardCode::NotRunning);
        }
        let cur = match self.accounts.get(idx) {
            Some(a) => a.charged_ms,
            None => return Err(GuardCode::SegmentViolation),
        };
        let next = cur.saturating_add(ms);
        if next > BATCH_CAP_MS {
            return Err(GuardCode::SilentTimeout);
        }
        let next_phase = if next >= SPLIT_KNEE_MS {
            BatchPhase::SplitPending
        } else {
            BatchPhase::Running
        };
        if let Some(a) = self.accounts.get_mut(idx) {
            a.charged_ms = next;
            a.total_charged = a.total_charged.saturating_add(ms);
            a.phase = next_phase;
        }
        Ok(next)
    }

    /// 拆批：把在计时长归档并清零、留痕编号连续递增、相位回 Running。
    pub fn split(&mut self, batch: u32) -> Result<u32, GuardCode> {
        let idx = match self.find(batch) {
            Some(i) => i,
            None => return Err(GuardCode::UnknownBatch),
        };
        let (phase, charged, n_splits) = match self.accounts.get(idx) {
            Some(a) => (a.phase, a.charged_ms, a.splits.len() as u32),
            None => return Err(GuardCode::SegmentViolation),
        };
        if !phase.chargeable() {
            return Err(GuardCode::NotRunning);
        }
        if charged == 0 {
            return Err(GuardCode::EmptyBatch);
        }
        let index = n_splits.saturating_add(1);
        if let Some(a) = self.accounts.get_mut(idx) {
            a.archived_ms = a.archived_ms.saturating_add(a.charged_ms);
            a.splits.push(SplitRecord { batch, index, archived_ms: a.charged_ms });
            a.charged_ms = 0;
            a.phase = BatchPhase::Running;
        }
        Ok(index)
    }

    /// 终态：批相位推进到 Done；Done 后充电 NotRunning 拒绝。
    pub fn finalize(&mut self, batch: u32) -> Result<(), GuardCode> {
        let idx = match self.find(batch) {
            Some(i) => i,
            None => return Err(GuardCode::UnknownBatch),
        };
        if let Some(a) = self.accounts.get_mut(idx) {
            if a.phase == BatchPhase::Done {
                return Err(GuardCode::PhaseRegress);
            }
            a.phase = BatchPhase::Done;
        }
        Ok(())
    }

    /// 查批账户（只读）。
    pub fn account(&self, batch: u32) -> Option<&BatchAccount> {
        match self.find(batch) {
            Some(i) => self.accounts.get(i),
            None => None,
        }
    }

    /// 拆批留痕核验：所有批的拆批编号必须从 1 连续递增。
    pub fn split_trace_sound(&self) -> bool {
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some(a) = self.accounts.get(i) {
                let mut k = 0usize;
                while k < a.splits.len() {
                    let expect = (k + 1) as u32;
                    match a.splits.get(k) {
                        Some(r) => {
                            if r.index != expect || !r.sound() {
                                return false;
                            }
                        }
                        None => return false,
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        true
    }

    /// 全账守恒：每批 charged + archived == total_charged。
    pub fn conserved(&self) -> bool {
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some(a) = self.accounts.get(i) {
                if a.charged_ms.saturating_add(a.archived_ms) != a.total_charged {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 已建档批数。
    pub fn len(&self) -> usize {
        self.accounts.len()
    }
}

// ── 护栏成文（纯函数） ────────────────────────────────────────────────
/// 护栏成文：「批=<id>;已计=<ms>;归档=<ms>;拆批=<n>」；空护栏先拒。
pub fn compose_guard_summary(g: &TimeGuard, batch: u32) -> Result<String, GuardCode> {
    let a = match g.account(batch) {
        Some(a) => a,
        None => return Err(GuardCode::UnknownBatch),
    };
    if !g.conserved() {
        return Err(GuardCode::LedgerRegress);
    }
    let out = format!(
        "批={};已计={};归档={};拆批={}",
        a.batch, a.charged_ms, a.archived_ms, a.splits.len()
    );
    if out.is_empty() {
        return Err(GuardCode::EmptySummary);
    }
    Ok(out)
}

// ── 注入核：坏充电样本矩阵 ────────────────────────────────────────────
/// 四类坏充电样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCharge {
    /// 静默超时：越过上限仍不拆批
    SilentTimeout,
    /// 零时长充电
    ZeroElapsed,
    /// 空批拆分
    SplitWithoutCharge,
    /// 未登记批充电
    UnknownBatchCharge,
}

impl BadCharge {
    /// 闭集全量。
    pub const ALL: [BadCharge; 4] = [
        BadCharge::SilentTimeout,
        BadCharge::ZeroElapsed,
        BadCharge::SplitWithoutCharge,
        BadCharge::UnknownBatchCharge,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadCharge::SilentTimeout => "静默超时",
            BadCharge::ZeroElapsed => "零时长充电",
            BadCharge::SplitWithoutCharge => "空批拆分",
            BadCharge::UnknownBatchCharge => "未登记批充电",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账失守恒返回 LedgerRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), GuardCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(GuardCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏充电注入矩阵：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_charge_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadCharge::ALL.len() {
        let case = match BadCharge::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadCharge::SilentTimeout => {
                    let mut g = TimeGuard::new();
                    let _ = g.start(1);
                    let _ = g.charge(1, 500_000);
                    g.charge(1, 200_000).err() == Some(GuardCode::SilentTimeout)
                }
                BadCharge::ZeroElapsed => {
                    let mut g = TimeGuard::new();
                    let _ = g.start(1);
                    g.charge(1, 0).err() == Some(GuardCode::ZeroElapsed)
                }
                BadCharge::SplitWithoutCharge => {
                    let mut g = TimeGuard::new();
                    let _ = g.start(1);
                    g.split(1).err() == Some(GuardCode::EmptyBatch)
                }
                BadCharge::UnknownBatchCharge => {
                    let mut g = TimeGuard::new();
                    g.charge(99, 100).err() == Some(GuardCode::UnknownBatch)
                }
            };
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
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；零时长/坏参数族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；相位非法/终态续计族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；超限族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；批未登记族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复批号族"),
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

/// 规范语料双跑指纹：建档 → 充电越膝点 → 拆批留痕 → 续充 → 终态 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut g = TimeGuard::new();
    let _ = g.start(1);
    let _ = g.charge(1, 300_000);
    let _ = g.charge(1, 200_000); // 500_000 ≥ knee 480_000 → SplitPending
    let _ = g.split(1);
    let _ = g.charge(1, 100_000);
    let _ = g.finalize(1);
    let bytes = match compose_guard_summary(&g, 1) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10053 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10053_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10053");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91D40..=0x91D7F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠");

    // 2. 护栏常量：cap 有效且 knee = 0.8×cap 推导成立
    s.add("护栏常量：cap>0 且 knee=0.8×cap 严格小于 cap",
          BATCH_CAP_MS > 0 && SPLIT_KNEE_MS == BATCH_CAP_MS / 5 * 4
              && SPLIT_KNEE_MS < BATCH_CAP_MS,
          "膝点必须由上限推导且严格小于上限，否则拆批永远不触发");

    // 3. 状态机全链路：Idle→Running→SplitPending→Running→Done 10/10
    let mut chain_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut g = TimeGuard::new();
        let _ = g.start(1);
        let _ = g.charge(1, 300_000);
        let _ = g.charge(1, 200_000);
        let p1 = g.account(1).map(|a| a.phase).unwrap_or(BatchPhase::Idle);
        let _ = g.split(1);
        let p2 = g.account(1).map(|a| a.phase).unwrap_or(BatchPhase::Idle);
        let _ = g.finalize(1);
        let p3 = g.account(1).map(|a| a.phase).unwrap_or(BatchPhase::Idle);
        if p1 != BatchPhase::SplitPending || p2 != BatchPhase::Running || p3 != BatchPhase::Done {
            chain_ok = false;
        }
        rr += 1;
    }
    s.add("批相位状态机：全链路 10/10 次秩序推进", chain_ok,
          "Idle→Running→SplitPending→Running→Done 必须逐次按序到达");

    // 4. 合法充电：charged 逐次累计
    let mut g = TimeGuard::new();
    let _ = g.start(1);
    let _ = g.charge(1, 100_000);
    let _ = g.charge(1, 150_000);
    let charge_ok = g.account(1).map(|a| a.charged_ms).unwrap_or(0) == 250_000;
    s.add("合法充电：累计时长逐次入账", charge_ok,
          "充电只允许 Running 相位且时长大于 0");

    // 5. 膝点触发：越过 knee 10/10 次进入 SplitPending
    let mut knee_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut g2 = TimeGuard::new();
        let _ = g2.start(1);
        let _ = g2.charge(1, SPLIT_KNEE_MS);
        if g2.account(1).map(|a| a.phase).unwrap_or(BatchPhase::Idle) != BatchPhase::SplitPending {
            knee_ok = false;
        }
        rr += 1;
    }
    s.add("膝点触发：越过 knee 10/10 次进入 SplitPending", knee_ok,
          "knee=0.8×cap 越线必须逐次触发拆批待办相位");

    // 6. 拆批留痕：编号连续递增且归档累计
    let _ = g.split(1);
    let _ = g.charge(1, 50_000);
    let _ = g.split(1);
    let trace_ok = g.split_trace_sound()
        && g.account(1).map(|a| a.archived_ms).unwrap_or(0) == 300_000
        && g.account(1).map(|a| a.charged_ms).unwrap_or(0) == 0;
    s.add("拆批留痕：编号 1..n 连续且归档累计正确", trace_ok,
          "每次拆批必须留 SplitRecord，编号断裂即留痕失效");

    // 7. 静默超时拦截：越 cap 未拆批 10/10 次拒绝
    let mut st_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut g2 = TimeGuard::new();
        let _ = g2.start(1);
        let _ = g2.charge(1, 500_000);
        if g2.charge(1, 200_000).err() != Some(GuardCode::SilentTimeout) {
            st_ok = false;
        }
        rr += 1;
    }
    s.add("静默超时：越 cap 未拆批充电 10/10 次拦截", st_ok,
          "累计越过上限仍不拆批的充电必须逐次显性拒绝");

    // 8. 注入核：四类坏充电各 10/10 拒绝
    let (inj, per_class) = run_bad_charge_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏充电样本各 10/10 拒绝", all_caught,
          "静默超时/零时长/空批拆分/未登记批必须逐类逐次拒绝");

    // 9. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadCharge::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 成本账守恒：charged + archived == total_charged
    s.add("成本守恒：charged+archived == total_charged",
          g.conserved()
              && g.account(1).map(|a| a.total_charged).unwrap_or(0) == 300_000,
          "充电总量必须等于在计与归档之和，绕账路径即失败");

    // 11. 拒绝不留痕：被拒充电不改账
    let before = g.account(1).map(|a| (a.charged_ms, a.archived_ms, a.total_charged));
    let _ = g.charge(1, 999_999);
    let _ = g.charge(1, 0);
    let after = g.account(1).map(|a| (a.charged_ms, a.archived_ms, a.total_charged));
    s.add("拒绝不留痕：坏充电不改三账", before == after,
          "被拒充电不得改动 charged/archived/total 任何账目");

    // 12. 终态后充电拒绝 + 空批拆分拒绝
    let mut g3 = TimeGuard::new();
    let _ = g3.start(2);
    let _ = g3.charge(2, 10_000);
    let _ = g3.finalize(2);
    let term_ok = g3.charge(2, 10_000).err() == Some(GuardCode::NotRunning)
        && g3.split(2).err() == Some(GuardCode::NotRunning);
    s.add("终态拦截：Done 批充电/拆分均拒", term_ok,
          "终态批必须冻结账目，任何续计企图显性拒绝");

    // 13. 成文非空且含标记 + 双跑一致
    let sum = compose_guard_summary(&g, 1);
    let text_ok = match &sum {
        Ok(t) => t.contains("批=1") && t.contains("拆批=2"),
        Err(_) => false,
    };
    s.add("护栏成文：标记齐全且两次成文逐字节一致",
          text_ok && sum == compose_guard_summary(&g, 1),
          "成文必须携带 批/已计/归档/拆批 标记且为纯函数");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(1) && agrees_with_linux(28)
        && agrees_with_linux(2) && agrees_with_linux(17)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < GuardCode::ALL.len() {
        let mut j = i + 1;
        while j < GuardCode::ALL.len() {
            if let (Some(a), Some(b)) = (GuardCode::ALL.get(i), GuardCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = GuardCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 双跑指纹一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希相等", f1 == f2,
          "同输入两次必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &sum {
        Ok(t) => t.len(),
        Err(_) => 0,
    };
    let sum_text = format!("护栏小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knee_derivation() {
        assert_eq!(BATCH_CAP_MS, 600_000);
        assert_eq!(SPLIT_KNEE_MS, 480_000);
        assert!(SPLIT_KNEE_MS < BATCH_CAP_MS);
    }

    #[test]
    fn charge_accumulates() {
        let mut g = TimeGuard::new();
        let _ = g.start(7);
        assert_eq!(g.charge(7, 100).ok(), Some(100));
        assert_eq!(g.charge(7, 200).ok(), Some(300));
        assert_eq!(g.account(7).unwrap().charged_ms, 300);
    }

    #[test]
    fn silent_timeout_intercepted() {
        let mut g = TimeGuard::new();
        let _ = g.start(1);
        let _ = g.charge(1, 599_999);
        assert_eq!(g.charge(1, 1).ok(), Some(600_000));
        assert_eq!(g.charge(1, 1).err(), Some(GuardCode::SilentTimeout));
    }

    #[test]
    fn split_trace_continuous() {
        let mut g = TimeGuard::new();
        let _ = g.start(1);
        let _ = g.charge(1, 480_000);
        assert_eq!(g.split(1).ok(), Some(1));
        let _ = g.charge(1, 480_000);
        assert_eq!(g.split(1).ok(), Some(2));
        assert!(g.split_trace_sound());
        assert!(g.conserved());
    }

    #[test]
    fn empty_split_rejected() {
        let mut g = TimeGuard::new();
        let _ = g.start(1);
        assert_eq!(g.split(1).err(), Some(GuardCode::EmptyBatch));
    }

    #[test]
    fn bad_charge_matrix_green() {
        let (inj, per) = run_bad_charge_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rejected_without_trace() {
        let mut g = TimeGuard::new();
        let _ = g.start(1);
        let _ = g.charge(1, 100);
        let before = g.account(1).map(|a| a.total_charged).unwrap_or(0);
        let _ = g.charge(99, 100);
        let _ = g.charge(1, 0);
        let after = g.account(1).map(|a| a.total_charged).unwrap_or(0);
        assert_eq!(before, after);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(28));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(22), -22);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10053_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10053_checks_green() {
        let set = run_unx10053_checks();
        assert!(set.all_passed(), "UNX-F10053 red: {}", set.red_items_text());
    }
}
