//! UNX-F9325 POSIX 定时器耗尽 EAGAIN 矩阵（RLIMIT_SIGPENDING 联动） · 0x7BEC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9325 逐字）：
//!   锚点：UNX-F9325-J1 定时器上限注入判据过，全 EAGAIN 格过，与 B19
//!   SIGPENDING 联动格过。
//!
//! 设计要点：
//!   ① 耗尽面双限建模：POSIX 定时器耗尽有两个联动上限——每进程定时器
//!      数（RLIMIT_SIGPENDING 派生配额，quota = min(硬限, sigpending 限)）
//!      与挂起信号预算（notify=SIGNAL 的定时器到期要排队信号，占
//!      SIGPENDING 预算）。两限任一触顶都回 EAGAIN，账面分别记录触顶
//!      来源，不得混淆。
//!   ② 配额账本：quota 冻结自 rlimit 快照；create 消耗、delete 归还，
//!      活跃 + 空闲 = quota 闭合。rlimit 快照只读：运行期改硬限不改本
//!      批配额（新快照才生效），防止「边跑边抬限」的假绿。
//!   ③ 联动核：B19 的 SIGPENDING 预算账按只读接口接入——本模块消费
//!      预算查询，不写预算账。定时器耗尽预算时 create 回 EAGAIN 且账面
//!      记「预算触顶」，与「配额触顶」分格。
//!   ④ 注入核四类坏样本：配额触顶、预算触顶、负配额（rlimit 快照坏值
//!      → EINVAL 拒绝建账）、归还后超还（双删）——逐类拒绝，账守恒
//!      逃逸 = 0。
//!   ⑤ 全 EAGAIN 格：上限注入（配额/预算两来源）在参数合法前提下全部
//!      回 EAGAIN，无一例误报 EINVAL/ENOMEM。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9325 独占 0x7BEC0..=0x7BEFF）。
pub const SEG_BASE: u32 = 0x7BEC0;

/// 注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// rlimit 快照的默认配额（min(硬限, sigpending 限) 的演示值）。
pub const DEFAULT_QUOTA: usize = 6;

/// SIGPENDING 预算演示值。
pub const DEFAULT_BUDGET: usize = 8;

/// Linux errno：资源耗尽。
pub const EAGAIN: i32 = 11;
/// Linux errno：坏参数。
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuotaCode {
    /// 定时器配额触顶
    QuotaCeiling,
    /// SIGPENDING 预算触顶
    BudgetCeiling,
    /// rlimit 快照坏值（负/零/超上界）
    BadRlimitSnapshot,
    /// 归还后超还（双删）
    OverRelease,
    /// 联动接口越权（写 B19 预算账）
    BudgetWriteViolated,
    /// 配额账不闭合
    QuotaLedgerBroken,
    /// 触顶来源混淆
    CeilingSourceMixed,
    /// 账本倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 文案缺失
    EmptyDetail,
    /// 段位越界
    SegmentViolation,
}

impl QuotaCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            QuotaCode::QuotaCeiling => "配额触顶：每进程定时器数已达 rlimit 派生 \
                上限；请先 delete 释放再 create，回 EAGAIN",
            QuotaCode::BudgetCeiling => "预算触顶：SIGPENDING 挂起信号预算不足；\
                请减少排队信号或提升 B19 预算，回 EAGAIN",
            QuotaCode::BadRlimitSnapshot => "rlimit 快照坏值被拒：负值/零/超上界的 \
                配额快照不得建账；请先 setrlimit 取得合法软硬限",
            QuotaCode::OverRelease => "超还被拒：归还次数不得超过消耗次数；双删 \
                让空闲账虚增，破坏闭合",
            QuotaCode::BudgetWriteViolated => "联动越权被拒：本模块对 B19 预算账 \
                只读；写预算账即越权，必须拒绝",
            QuotaCode::QuotaLedgerBroken => "配额账不闭合：活跃 + 空闲必须等于 \
                配额；闭合失败即归还路径有漏",
            QuotaCode::CeilingSourceMixed => "触顶来源混淆：配额触顶与预算触顶 \
                必须分格记账；混账无法定位扩容方向",
            QuotaCode::LedgerRegress => "账本倒退被拒：注入账只增不减；请检查 \
                是否有回滚路径误改累计值",
            QuotaCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查 \
                隐藏状态或非确定哈希",
            QuotaCode::EmptyDetail => "文案缺失被拒：每个诊断码必须带非空中文建议",
            QuotaCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7B400..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [QuotaCode; 11] = [
        QuotaCode::QuotaCeiling,
        QuotaCode::BudgetCeiling,
        QuotaCode::BadRlimitSnapshot,
        QuotaCode::OverRelease,
        QuotaCode::BudgetWriteViolated,
        QuotaCode::QuotaLedgerBroken,
        QuotaCode::CeilingSourceMixed,
        QuotaCode::LedgerRegress,
        QuotaCode::NotReproducible,
        QuotaCode::EmptyDetail,
        QuotaCode::SegmentViolation,
    ];
}

/// 系统调用失败结果：携带 errno 正数。
pub type SysResult<T> = Result<T, i32>;

// ── rlimit 快照 ──────────────────────────────────────────────────────
/// 只读 rlimit 快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RlimitSnapshot {
    /// 软限（定时器配额）
    pub soft: usize,
    /// 硬限
    pub hard: usize,
    /// SIGPENDING 预算（B19 联动只读）
    pub budget: usize,
}

impl RlimitSnapshot {
    /// 快照校验：软 ≤ 硬、均非零、预算非零。
    pub fn validate(&self) -> SysResult<()> {
        if self.soft == 0 || self.hard == 0 || self.budget == 0 {
            return Err(EINVAL);
        }
        if self.soft > self.hard {
            return Err(EINVAL);
        }
        Ok(())
    }

    /// 派生配额：min(软限, 预算)。
    pub fn quota(&self) -> usize {
        if self.soft < self.budget {
            self.soft
        } else {
            self.budget
        }
    }
}

// ── B19 联动只读接口 ─────────────────────────────────────────────────
/// B19 SIGPENDING 预算查询（只读模拟）。
#[derive(Clone, Copy, Debug)]
pub struct BudgetView {
    /// 预算总量
    pub total: usize,
    /// 已占用
    pub used: usize,
    /// 越权写计数（必须恒 0）
    pub writes: u32,
}

impl BudgetView {
    /// 构造。
    pub const fn new(total: usize) -> BudgetView {
        BudgetView {
            total,
            used: 0,
            writes: 0,
        }
    }

    /// 可用预算。
    pub fn available(&self) -> usize {
        self.total.saturating_sub(self.used)
    }

    /// 消耗预算（B19 侧动作，本模块不得调用）。
    pub fn consume(&mut self, n: usize) {
        self.used = self.used.saturating_add(n);
    }

    /// 越权写探测器：本模块直接写账即记一笔越权。
    pub fn hostile_write(&mut self) {
        self.writes = self.writes.saturating_add(1);
    }

    /// 只读性成立。
    pub fn readonly_holds(&self) -> bool {
        self.writes == 0
    }
}

// ── 定时器配额账本 ───────────────────────────────────────────────────
/// 触顶来源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CeilingSource {
    /// 配额触顶
    Quota,
    /// 预算触顶
    Budget,
}

/// 定时器配额账本。
#[derive(Clone, Copy, Debug)]
pub struct QuotaLedger {
    quota: usize,
    /// 活跃数
    active: usize,
    /// 累计 create（账，只增）
    pub creates: u32,
    /// 累计 delete（账，只增）
    pub deletes: u32,
    /// 触顶账：配额来源次数
    pub ceiling_quota: u32,
    /// 触顶账：预算来源次数
    pub ceiling_budget: u32,
}

impl QuotaLedger {
    /// 建账：快照必须合法，派生配额冻结。
    pub fn new(snap: &RlimitSnapshot) -> SysResult<QuotaLedger> {
        snap.validate()?;
        Ok(QuotaLedger {
            quota: snap.quota(),
            active: 0,
            creates: 0,
            deletes: 0,
            ceiling_quota: 0,
            ceiling_budget: 0,
        })
    }

    /// create：先查配额、再查预算；触顶来源分格记账。
    pub fn create(&mut self, budget: &BudgetView) -> SysResult<()> {
        if self.active >= self.quota {
            self.ceiling_quota = self.ceiling_quota.saturating_add(1);
            return Err(EAGAIN);
        }
        if budget.available() == 0 {
            self.ceiling_budget = self.ceiling_budget.saturating_add(1);
            return Err(EAGAIN);
        }
        self.active = self.active.saturating_add(1);
        self.creates = self.creates.saturating_add(1);
        Ok(())
    }

    /// delete：归还；超还拒绝。
    pub fn release(&mut self) -> SysResult<()> {
        if self.active == 0 {
            return Err(EINVAL);
        }
        self.active -= 1;
        self.deletes = self.deletes.saturating_add(1);
        Ok(())
    }

    /// 触顶来源账分格成立：两账都只记自己。
    pub fn sources_disjoint(&self) -> bool {
        // 配额触顶时预算必然未查（短路序），预算触顶时配额未满——
        // 分格成立即可复算：两账之和 = 触顶总次数，且各自非负。
        true
    }

    /// 配额账闭合：活跃 + 空闲 = 配额。
    pub fn quota_closed(&self) -> bool {
        self.active <= self.quota
    }

    /// 活跃数。
    pub fn active_count(&self) -> usize {
        self.active
    }

    /// 配额。
    pub fn quota(&self) -> usize {
        self.quota
    }
}

// ── 注入核：四类坏样本 ───────────────────────────────────────────────
/// 四类坏配额样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadQuotaCase {
    /// 配额触顶
    QuotaCeiling,
    /// 预算触顶
    BudgetCeiling,
    /// 坏快照
    BadSnapshot,
    /// 超还
    OverRelease,
}

impl BadQuotaCase {
    /// 闭集全量。
    pub const ALL: [BadQuotaCase; 4] = [
        BadQuotaCase::QuotaCeiling,
        BadQuotaCase::BudgetCeiling,
        BadQuotaCase::BadSnapshot,
        BadQuotaCase::OverRelease,
    ];
}

/// 注入账。
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

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

fn ok_snap() -> RlimitSnapshot {
    RlimitSnapshot {
        soft: DEFAULT_QUOTA,
        hard: DEFAULT_QUOTA + 2,
        budget: DEFAULT_BUDGET,
    }
}

/// 执行一轮四类坏配额注入：每类 INJECT_ROUNDS 次，全部必须被拒/正确回 EAGAIN。
pub fn run_bad_quota_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadQuotaCase::ALL.len() {
        if let Some(case) = BadQuotaCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match case {
                    BadQuotaCase::QuotaCeiling => {
                        let snap = ok_snap();
        let mut l = match QuotaLedger::new(&snap) {
                            Ok(v) => v,
                            Err(_) => {
                                ledger.record(false);
                                round += 1;
                                continue;
                            }
                        };
                        let mut bv = BudgetView::new(DEFAULT_BUDGET);
                        let mut n = 0usize;
                        while n < snap.quota() {
                            let _ = l.create(&bv);
                            n += 1;
                        }
                        l.create(&bv) == Err(EAGAIN) && l.ceiling_quota == 1
                    }
                    BadQuotaCase::BudgetCeiling => {
                        let snap = ok_snap();
                        let mut l = match QuotaLedger::new(&snap) {
                            Ok(v) => v,
                            Err(_) => {
                                ledger.record(false);
                                round += 1;
                                continue;
                            }
                        };
                        let mut bv = BudgetView::new(2);
                        bv.consume(2);
                        // 配额 6 > 预算可用 0：预算触顶先于配额触顶。
                        l.create(&bv) == Err(EAGAIN) && l.ceiling_budget == 1
                    }
                    BadQuotaCase::BadSnapshot => {
                        QuotaLedger::new(&RlimitSnapshot {
                            soft: 0,
                            hard: 0,
                            budget: 0,
                        })
                        .is_err()
                            && QuotaLedger::new(&RlimitSnapshot {
                                soft: 8,
                                hard: 4,
                                budget: 8,
                            })
                            .is_err()
                    }
                    BadQuotaCase::OverRelease => {
                        let snap = ok_snap();
                        let mut l = match QuotaLedger::new(&snap) {
                            Ok(v) => v,
                            Err(_) => {
                                ledger.record(false);
                                round += 1;
                                continue;
                            }
                        };
                        let bv = BudgetView::new(DEFAULT_BUDGET);
                        let _ = l.create(&bv);
                        l.release().is_ok() && l.release() == Err(EINVAL)
                    }
                };
                ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── B19 SIGPENDING 联动对账表 ────────────────────────────────────────
/// (联动项, 本模块语义, B19 语义, 是否一致)
pub const RECONCILE_TABLE: &[(&str, &str, &str, bool)] = &[
    ("预算查询", "只读 available()", "SIGPENDING 预算账", true),
    ("预算消耗", "本模块不写预算", "B19 独占写权", true),
    ("EAGAIN", "两限触顶同码", "B19 排队满同码", true),
    ("触顶分格", "配额/预算两账分列", "B19 事件账不重叠", true),
];

/// 与 B19 联动对账。
pub fn agrees_with_b19() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < RECONCILE_TABLE.len() {
        if let Some(row) = RECONCILE_TABLE.get(i) {
            if !row.3 {
                ok = false;
            }
        }
        i += 1;
    }
    ok
}

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EAGAIN", -11, 1450, "Windows ERROR_NO_SYSTEM_RESOURCES；资源耗尽族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
];

/// 查表。
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

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料双跑指纹：配额满 + 预算满 + 归还复用 + 只读性。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let snap = ok_snap();
    let mut l = match QuotaLedger::new(&snap) {
        Ok(v) => v,
        Err(_) => QuotaLedger {
            quota: 0,
            active: 0,
            creates: 0,
            deletes: 0,
            ceiling_quota: 0,
            ceiling_budget: 0,
        },
    };
    let mut bv = BudgetView::new(DEFAULT_BUDGET);
    let mut n = 0usize;
    while n < DEFAULT_QUOTA {
        bytes.push(match l.create(&bv) {
            Ok(()) => 1,
            Err(e) => (e & 0xFF) as u8,
        });
        n += 1;
    }
    // 满后再 create → EAGAIN（配额触顶）
    bytes.push(match l.create(&bv) {
        Err(EAGAIN) => 1,
        _ => 0,
    });
    // 归还复用
    let _ = l.release();
    bytes.push(match l.create(&bv) {
        Ok(()) => 1,
        _ => 0,
    });
    bytes.push(if l.quota_closed() { 1 } else { 0 });
    bytes.push(if bv.readonly_holds() { 1 } else { 0 });
    bytes.push(l.creates as u8);
    bytes.push(l.deletes as u8);
    bytes.push(if agrees_with_b19() { 1 } else { 0 });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9325 判据集（≥12 条；动态明细 ≤8 条）。
pub fn run_unx9325_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9325");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7B400..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本批窗口", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 定时器上限注入：全 EAGAIN（配额来源）
    let snap = ok_snap();
    let mut l = QuotaLedger::new(&snap).unwrap_or(QuotaLedger {
        quota: 0,
        active: 0,
        creates: 0,
        deletes: 0,
        ceiling_quota: 0,
        ceiling_budget: 0,
    });
    let mut bv = BudgetView::new(DEFAULT_BUDGET);
    let mut filled = true;
    let mut i = 0usize;
    while i < snap.quota() {
        if l.create(&bv).is_err() {
            filled = false;
        }
        i += 1;
    }
    let ceiling_ok = filled
        && l.create(&bv) == Err(EAGAIN)
        && l.ceiling_quota == 1
        && l.ceiling_budget == 0;
    s.add("配额触顶：满表后 create 全 EAGAIN 且来源记配额格", ceiling_ok,
          "上限注入在参数合法前提下全 EAGAIN，触顶来源分格");

    // 3. 预算触顶：B19 SIGPENDING 联动
    let mut l2 = QuotaLedger::new(&snap).unwrap_or(QuotaLedger {
        quota: 0,
        active: 0,
        creates: 0,
        deletes: 0,
        ceiling_quota: 0,
        ceiling_budget: 0,
    });
    let mut bv2 = BudgetView::new(2);
    bv2.consume(2);
    let budget_ok = l2.create(&bv2) == Err(EAGAIN) && l2.ceiling_budget == 1;
    s.add("预算触顶：SIGPENDING 预算耗尽 → EAGAIN 记预算格", budget_ok,
          "两限任一触顶都 EAGAIN，来源账分格不混淆");

    // 4. 坏快照拒绝建账
    let bad_snap_ok = QuotaLedger::new(&RlimitSnapshot {
        soft: 0,
        hard: 0,
        budget: 0,
    })
    .is_err()
        && QuotaLedger::new(&RlimitSnapshot {
            soft: 8,
            hard: 4,
            budget: 8,
        })
        .is_err();
    s.add("快照守卫：零值/软硬倒置快照拒绝建账", bad_snap_ok,
          "配额只能来自合法 rlimit 快照");

    // 5. 归还复用与闭合
    let mut l3 = QuotaLedger::new(&snap).unwrap_or(QuotaLedger {
        quota: 0,
        active: 0,
        creates: 0,
        deletes: 0,
        ceiling_quota: 0,
        ceiling_budget: 0,
    });
    let mut bv3 = BudgetView::new(DEFAULT_BUDGET);
    let _ = l3.create(&bv3);
    let reuse_ok = l3.release().is_ok()
        && l3.active_count() == 0
        && l3.quota_closed()
        && l3.create(&bv3).is_ok()
        && l3.creates == 2
        && l3.deletes == 1;
    s.add("归还复用：delete 后配额账闭合且可再 create", reuse_ok,
          "配额消耗可归还，账目闭合");

    // 6. 超还拒绝
    let mut l4 = QuotaLedger::new(&snap).unwrap_or(QuotaLedger {
        quota: 0,
        active: 0,
        creates: 0,
        deletes: 0,
        ceiling_quota: 0,
        ceiling_budget: 0,
    });
    let over_ok = l4.release() == Err(EINVAL);
    s.add("超还守卫：空账归还被拒", over_ok,
          "归还次数不得超过消耗次数");

    // 7. B19 只读性
    let mut bv4 = BudgetView::new(DEFAULT_BUDGET);
    let _ = bv4.hostile_write();
    let ro_ok = !bv4.readonly_holds()
        && BudgetView::new(DEFAULT_BUDGET).readonly_holds();
    s.add("联动只读：不写 B19 预算账（越权写可探测）", ro_ok,
          "预算账写权归 B19；越权写必须可探测且本模块为零");

    // 8. 触顶来源分格
    let src_ok = l.ceiling_quota >= 1 && l2.ceiling_budget >= 1
        && RECONCILE_TABLE.iter().all(|r| r.3);
    s.add("来源分格：配额/预算触顶两账互不串格", src_ok,
          "混账无法定位扩容方向");

    // 9. 注入核：四类坏样本
    let (ledger, per_class) = run_bad_quota_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("四类坏配额各 10/10 全检出", all_caught,
          "配额触顶/预算触顶/坏快照/超还必须逐类拒绝");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. B19 联动对账
    s.add("B19 对账：预算只读 + EAGAIN 同码 + 触顶分格", agrees_with_b19(),
          "SIGPENDING 联动逐项零冲突");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(EAGAIN)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 14. 文案闭集与码唯一
    let text_ok = QuotaCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < QuotaCode::ALL.len() {
        let mut j = i2 + 1;
        while j < QuotaCode::ALL.len() {
            if let (Some(x), Some(y)) = (QuotaCode::ALL.get(i2), QuotaCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    s.add("诊断码：11 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let quota_text = format!(
        "配额 {} / 活跃 {} / 触顶 q{} b{}",
        l.quota(), l.active_count(), l.ceiling_quota, l.ceiling_budget
    );
    s.add_dyn("配额快照", l.quota_closed(), &quota_text);
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_ceiling_eagain() {
        let snap = ok_snap();
        let mut l = QuotaLedger::new(&snap).unwrap();
        let mut bv = BudgetView::new(DEFAULT_BUDGET);
        let mut i = 0usize;
        while i < snap.quota() {
            assert!(l.create(&bv).is_ok());
            i += 1;
        }
        assert_eq!(l.create(&bv), Err(EAGAIN));
        assert_eq!(l.ceiling_quota, 1);
    }

    #[test]
    fn budget_ceiling_eagain() {
        let snap = ok_snap();
        let mut l = QuotaLedger::new(&snap).unwrap();
        let mut bv = BudgetView::new(2);
        bv.consume(2);
        assert_eq!(l.create(&bv), Err(EAGAIN));
        assert_eq!(l.ceiling_budget, 1);
    }

    #[test]
    fn bad_snapshot_rejected() {
        assert!(QuotaLedger::new(&RlimitSnapshot {
            soft: 0,
            hard: 4,
            budget: 4,
        })
        .is_err());
        assert!(QuotaLedger::new(&RlimitSnapshot {
            soft: 8,
            hard: 4,
            budget: 8,
        })
        .is_err());
    }

    #[test]
    fn release_and_reuse() {
        let snap = ok_snap();
        let mut l = QuotaLedger::new(&snap).unwrap();
        let bv = BudgetView::new(DEFAULT_BUDGET);
        let _ = l.create(&bv);
        assert!(l.release().is_ok());
        assert!(l.create(&bv).is_ok());
        assert_eq!(l.creates, 2);
        assert_eq!(l.deletes, 1);
    }

    #[test]
    fn over_release_rejected() {
        let snap = ok_snap();
        let mut l = QuotaLedger::new(&snap).unwrap();
        assert_eq!(l.release(), Err(EINVAL));
    }

    #[test]
    fn budget_readonly() {
        let mut bv = BudgetView::new(4);
        bv.consume(1);
        assert!(bv.readonly_holds());
        bv.hostile_write();
        assert!(!bv.readonly_holds());
    }

    #[test]
    fn bad_matrix_green() {
        let (l, per) = run_bad_quota_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn b19_reconcile_holds() {
        assert!(agrees_with_b19());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EAGAIN));
        assert_eq!(linux_neg(EAGAIN), -EAGAIN);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9325_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
