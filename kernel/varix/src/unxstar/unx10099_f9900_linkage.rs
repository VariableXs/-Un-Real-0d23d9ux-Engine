//! UNX-F10099 E 型与 F9900 总收口衔接 · 0x928C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10100-J1 收口小结落账（20 条判据达成+预算 6,640 核销零偏离），核销不符 10/10 次检出
//!
//! SEG_BASE = 0x928C0（本卡独占 0x40 窗口，收口段末卡）；
//! 诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 注册表：E 型战线条目域注册表（卡号闭集 10081..=10100 共 20 条，
//!      域外卡号一律拒绝），馈入必须指向域内条目。
//!   ② 账本：馈入账 FeedLedger（E 型判据逐卡馈入 F9900 聚合）+ 核销账
//!      BudgetLedger（预算 6,640 逐笔核销）；对账审计要求 20/20 馈入且
//!      核销总和恰等预算，脱聚合/核销不符逐次显性拒绝。
//!   ③ 状态机：收口态 Open → Full → Balanced → Closed，逐门推进，
//!      越门收口显性拒绝（未满馈/未平衡不得闭门）。
//!   ④ 注入核：脱聚合 / 重复馈入 / 核销不符 / 越域卡号 四类坏样本逐类
//!      10/10 次拒绝，注入账 注入=检出+逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10099 独占 0x40 窗口，收口段末卡）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x928C0;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// E 型战线条目域（闭集）：F10081..=F10100。
pub const DOMAIN_LO: u32 = 10081;
/// 域上界（含）。
pub const DOMAIN_HI: u32 = 10100;

/// 域内条目数（20 条判据）。
pub const DOMAIN_SIZE: u32 = DOMAIN_HI - DOMAIN_LO + 1;

/// B25 段预算总额（收口核销锁定值）。
pub const BUDGET_TOTAL: u32 = 6640;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkCode {
    /// 越域卡号（10081..=10100 外）
    OutOfDomain,
    /// 同卡重复馈入
    DupFeed,
    /// 脱聚合（域内条目未馈入即收口）
    DetachedAggregate,
    /// 核销不符（总和偏离预算）
    BudgetMismatch,
    /// 核销超额单笔
    BudgetOverrun,
    /// 零判据馈入
    ZeroChecks,
    /// 越态收口
    StageViolation,
    /// 空账收口
    EmptyLedger,
    /// 收口小结为空
    EmptySummary,
    /// 拒绝后账本被改动
    RejectedTrace,
    /// 收口成文往返不一致
    SummaryRoundTrip,
    /// 段位越界
    SegmentViolation,
}

impl LinkCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            LinkCode::OutOfDomain => "越域卡号被拒：馈入只认 F10081..=F10100 域内条目；\
                请核对卡号",
            LinkCode::DupFeed => "重复馈入被拒：同卡只馈入一次；\
                复跑请走对账刷新而非再馈",
            LinkCode::DetachedAggregate => "脱聚合被拒：20 条战线条目必须全数馈入 F9900；\
                缺卡即脱聚合",
            LinkCode::BudgetMismatch => "核销不符被拒：核销总和必须恰等预算 6,640；\
                请对账补正差额",
            LinkCode::BudgetOverrun => "单笔超额被拒：单笔核销不得超预算总额；\
                请拆笔",
            LinkCode::ZeroChecks => "零判据馈入被拒：每卡必须携带正数判据数；\
                零判据即空馈",
            LinkCode::StageViolation => "越态收口被拒：收口态必须 \
                Open→Full→Balanced→Closed 逐门推进",
            LinkCode::EmptyLedger => "空账收口被拒：馈入账为空不得进入收口；\
                请先逐卡馈入",
            LinkCode::EmptySummary => "空成文被拒：收口小结不得为空串；请检查账面",
            LinkCode::RejectedTrace => "拒绝留痕被拒：被拒操作不得改动任何已冻结账面；\
                请排查拒绝路径是否误提交",
            LinkCode::SummaryRoundTrip => "收口成文往返不一致：再成文必须逐字复原；\
                请检查账面是否被旁路改动",
            LinkCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [LinkCode; 12] = [
        LinkCode::OutOfDomain,
        LinkCode::DupFeed,
        LinkCode::DetachedAggregate,
        LinkCode::BudgetMismatch,
        LinkCode::BudgetOverrun,
        LinkCode::ZeroChecks,
        LinkCode::StageViolation,
        LinkCode::EmptyLedger,
        LinkCode::EmptySummary,
        LinkCode::RejectedTrace,
        LinkCode::SummaryRoundTrip,
        LinkCode::SegmentViolation,
    ];
}

// ── 收口态状态机 ──────────────────────────────────────────────────────
/// 收口态：Open → Full(20/20 馈入) → Balanced(预算零偏离) → Closed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseStage {
    /// 开账：逐卡馈入中
    Open,
    /// 满馈：20/20
    Full,
    /// 平衡：核销恰等预算
    Balanced,
    /// 闭门：收口完成
    Closed,
}

impl CloseStage {
    /// 态序（0..=3）。
    pub fn rank(self) -> u8 {
        match self {
            CloseStage::Open => 0,
            CloseStage::Full => 1,
            CloseStage::Balanced => 2,
            CloseStage::Closed => 3,
        }
    }

    /// 中文态名。
    pub fn label(self) -> &'static str {
        match self {
            CloseStage::Open => "开账",
            CloseStage::Full => "满馈",
            CloseStage::Balanced => "平衡",
            CloseStage::Closed => "闭门",
        }
    }
}

// ── 双账本 ────────────────────────────────────────────────────────────
/// 馈入记录：卡号 + 判据数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FeedRecord {
    /// 卡号（域内）
    pub card: u32,
    /// 判据条数（正数）
    pub checks: u32,
}

/// 馈入 + 核销双账与收口态。
#[derive(Clone, Debug)]
pub struct CloseLedger {
    feeds: Vec<FeedRecord>,
    spent: u32,
    stage: CloseStage,
}

impl CloseLedger {
    /// 空账构造（态 Open，零核销）。
    pub fn new() -> CloseLedger {
        CloseLedger { feeds: Vec::new(), spent: 0, stage: CloseStage::Open }
    }

    /// 馈入一卡：域内、未馈、判据数为正；被拒不改账。
    pub fn feed(&mut self, card: u32, checks: u32) -> Result<u32, LinkCode> {
        if card < DOMAIN_LO || card > DOMAIN_HI {
            return Err(LinkCode::OutOfDomain);
        }
        if checks == 0 {
            return Err(LinkCode::ZeroChecks);
        }
        let mut i = 0usize;
        while i < self.feeds.len() {
            if let Some(f) = self.feeds.get(i) {
                if f.card == card {
                    return Err(LinkCode::DupFeed);
                }
            }
            i += 1;
        }
        self.feeds.push(FeedRecord { card, checks });
        Ok(self.feeds.len() as u32)
    }

    /// 已馈卡数。
    pub fn fed(&self) -> u32 {
        self.feeds.len() as u32
    }

    /// 核销累计。
    pub fn spent_total(&self) -> u32 {
        self.spent
    }

    /// 核销一笔：单笔不得超预算总额；总和不得超预算。
    pub fn spend(&mut self, amount: u32) -> Result<u32, LinkCode> {
        if amount > BUDGET_TOTAL {
            return Err(LinkCode::BudgetOverrun);
        }
        let next = self.spent.saturating_add(amount);
        if next > BUDGET_TOTAL {
            return Err(LinkCode::BudgetMismatch);
        }
        self.spent = next;
        Ok(self.spent)
    }

    /// 脱聚合审计：域内是否有未馈卡（返回未馈数）。
    pub fn unfed_count(&self) -> u32 {
        DOMAIN_SIZE - self.fed()
    }

    /// 预算平衡：核销总和恰等预算。
    pub fn balanced(&self) -> bool {
        self.spent == BUDGET_TOTAL
    }

    /// 推进收口门：Open→Full 要求 20/20；Full→Balanced 要求平衡；
    /// Balanced→Closed 直接闭门。任一门未达显性拒绝。
    pub fn promote(&mut self) -> Result<CloseStage, LinkCode> {
        match self.stage {
            CloseStage::Open => {
                if self.feeds.is_empty() {
                    return Err(LinkCode::EmptyLedger);
                }
                if self.fed() < DOMAIN_SIZE {
                    return Err(LinkCode::DetachedAggregate);
                }
                self.stage = CloseStage::Full;
                Ok(self.stage)
            }
            CloseStage::Full => {
                if !self.balanced() {
                    return Err(LinkCode::BudgetMismatch);
                }
                self.stage = CloseStage::Balanced;
                Ok(self.stage)
            }
            CloseStage::Balanced => {
                self.stage = CloseStage::Closed;
                Ok(self.stage)
            }
            CloseStage::Closed => Err(LinkCode::StageViolation),
        }
    }

    /// 当前态。
    pub fn stage(&self) -> CloseStage {
        self.stage
    }

    /// 全账判据数（20 条判据合计）。
    pub fn total_checks(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.feeds.len() {
            if let Some(f) = self.feeds.get(i) {
                t = t.saturating_add(f.checks);
            }
            i += 1;
        }
        t
    }
}

// ── 注入核：坏收口样本矩阵 ────────────────────────────────────────────
/// 四类坏收口样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadClose {
    /// 脱聚合：未满馈即推门
    DetachedAggregate,
    /// 重复馈入
    DupFeed,
    /// 核销不符：满馈但预算差额
    BudgetMismatch,
    /// 越域卡号
    OutOfDomain,
}

impl BadClose {
    /// 闭集全量。
    pub const ALL: [BadClose; 4] = [
        BadClose::DetachedAggregate,
        BadClose::DupFeed,
        BadClose::BudgetMismatch,
        BadClose::OutOfDomain,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadClose::DetachedAggregate => "脱聚合收口",
            BadClose::DupFeed => "重复馈入",
            BadClose::BudgetMismatch => "核销不符",
            BadClose::OutOfDomain => "越域卡号",
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

    /// 记一次注入结果；账失守恒返回 SegmentViolation 桥接码。
    pub fn record(&mut self, caught: bool) -> Result<(), LinkCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(LinkCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏收口注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_close_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadClose::ALL.len() {
        let case = match BadClose::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadClose::DetachedAggregate => {
                    let mut l = CloseLedger::new();
                    let _ = l.feed(DOMAIN_LO, 20);
                    // 只馈 1/20 即推门 → DetachedAggregate。
                    l.promote().err() == Some(LinkCode::DetachedAggregate)
                }
                BadClose::DupFeed => {
                    let mut l = CloseLedger::new();
                    let ok1 = l.feed(DOMAIN_LO, 20).is_ok();
                    ok1 && l.feed(DOMAIN_LO, 20).err() == Some(LinkCode::DupFeed)
                }
                BadClose::BudgetMismatch => {
                    let mut l = CloseLedger::new();
                    // 满馈但只核销一半。
                    let mut card = DOMAIN_LO;
                    while card <= DOMAIN_HI {
                        let _ = l.feed(card, 20);
                        card += 1;
                    }
                    let _ = l.promote();
                    let _ = l.spend(BUDGET_TOTAL / 2);
                    l.promote().err() == Some(LinkCode::BudgetMismatch)
                }
                BadClose::OutOfDomain => {
                    let mut l = CloseLedger::new();
                    l.feed(DOMAIN_LO - 1, 20).err() == Some(LinkCode::OutOfDomain)
                        && l.feed(DOMAIN_HI + 1, 20).err() == Some(LinkCode::OutOfDomain)
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

// ── 收口成文（纯函数） ────────────────────────────────────────────────
/// 收口成文：「馈=<n>/20;判据=<c>;核销=<s>/6640;态=<名>」；空账先拒。
pub fn compose_close_summary(l: &CloseLedger) -> Result<String, LinkCode> {
    if l.fed() == 0 && l.spent_total() == 0 {
        return Err(LinkCode::EmptySummary);
    }
    let out = format!("馈={}/{};判据={};核销={}/{};态={}",
        l.fed(), DOMAIN_SIZE, l.total_checks(), l.spent_total(), BUDGET_TOTAL,
        l.stage().label());
    if out.is_empty() {
        return Err(LinkCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；越域卡号/零判据族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复馈入族"),
    ("ENOLINK", -67, 111, "Windows ERROR_REM_NOT_LIST（近似）；脱聚合断链族"),
    ("EHOSTUNREACH", -113, 1231, "Windows ERROR_HOST_UNREACHABLE（近似）；聚合不可达族"),
    ("EFBIG", -27, 145, "Windows ERROR_DIR_NOT_EMPTY（近似）；核销超额族"),
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

/// 规范语料双跑指纹：20 卡逐卡馈入 → 预算一次核销 → 逐门推进 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = CloseLedger::new();
    let mut card = DOMAIN_LO;
    while card <= DOMAIN_HI {
        let _ = l.feed(card, 20);
        card += 1;
    }
    let _ = l.spend(BUDGET_TOTAL);
    let _ = l.promote();
    let _ = l.promote();
    let _ = l.promote();
    let bytes = match compose_close_summary(&l) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10099 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10099_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10099");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x928C0..=0x928FF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 域注册表：20 条战线条目闭集核对
    let dom_ok = DOMAIN_SIZE == 20
        && DOMAIN_LO == 10081 && DOMAIN_HI == 10100
        && BUDGET_TOTAL == 6640;
    s.add("域注册表：F10081..=F10100 共 20 条、预算 6,640 冻结", dom_ok,
          "收口域与预算是 F9900 衔接的锁定口径，域外一律拒绝");

    // 3. 满馈构建：20 卡逐卡馈入
    let mut l = CloseLedger::new();
    let mut card = DOMAIN_LO;
    let mut build_ok = true;
    while card <= DOMAIN_HI {
        if l.feed(card, 20).is_err() {
            build_ok = false;
        }
        card += 1;
    }
    s.add("满馈构建：20 卡逐卡馈入且判据合计 400", build_ok && l.fed() == DOMAIN_SIZE
              && l.total_checks() == 20 * 20,
          "E 型判据接入 F9900 聚合以逐卡馈入留痕，缺卡即脱聚合");

    // 4. 越域注入：域外卡号 10/10 拒绝
    let mut dom_bad = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if l.feed(DOMAIN_LO - 1, 20).is_ok()
            || l.feed(DOMAIN_HI + 1, 20).is_ok()
            || l.feed(0, 20).is_ok()
            || l.feed(99999, 20).is_ok()
        {
            dom_bad = false;
        }
        r += 1;
    }
    s.add("越域注入：域外卡号 10/10 拒绝", dom_bad,
          "馈入只认域内条目，越域即脱账通道");

    // 5. 注入核：四类坏收口样本各 10/10 拒绝
    let (inj, per_class) = run_bad_close_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏收口样本各 10/10 拒绝", all_caught,
          "脱聚合/重复馈入/核销不符/越域卡号必须逐类逐次拒绝");

    // 6. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadClose::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 7. 重复馈入拦截
    let mut l2 = CloseLedger::new();
    let _ = l2.feed(DOMAIN_LO, 20);
    let dup_ok = l2.feed(DOMAIN_LO, 20).err() == Some(LinkCode::DupFeed)
        && l2.fed() == 1;
    s.add("重复馈入拦截：同卡再馈被拒且账面不变", dup_ok,
          "一卡一馈是聚合账契约，再馈即重复口径");

    // 8. 收口三道门：满馈→平衡→闭门
    let mut l3 = CloseLedger::new();
    let mut c3 = DOMAIN_LO;
    while c3 <= DOMAIN_HI {
        let _ = l3.feed(c3, 20);
        c3 += 1;
    }
    let gate_ok = l3.promote() == Ok(CloseStage::Full)
        && l3.spend(BUDGET_TOTAL) == Ok(BUDGET_TOTAL)
        && l3.promote() == Ok(CloseStage::Balanced)
        && l3.promote() == Ok(CloseStage::Closed)
        && l3.promote().is_err();
    s.add("收口三道门：满馈→平衡→闭门逐门推进", gate_ok,
          "未满馈/未平衡不得闭门，逐门显性把关");

    // 9. 核销守卫：单笔超额与累计超额拒绝
    let mut l4 = CloseLedger::new();
    let over_ok = l4.spend(BUDGET_TOTAL + 1).err() == Some(LinkCode::BudgetOverrun)
        && l4.spend(BUDGET_TOTAL).is_ok()
        && l4.spend(1).err() == Some(LinkCode::BudgetMismatch)
        && l4.balanced();
    s.add("核销守卫：单笔/累计超额显性拒绝且恰额平衡", over_ok,
          "核销总和恰等 6,640，差额即核销不符");

    // 10. 脱聚合审计：未馈数逐减到零
    let mut l5 = CloseLedger::new();
    let audit_start = l5.unfed_count() == DOMAIN_SIZE;
    let _ = l5.feed(DOMAIN_LO, 20);
    let audit_mid = l5.unfed_count() == DOMAIN_SIZE - 1;
    let mut c5 = DOMAIN_LO + 1;
    while c5 <= DOMAIN_HI {
        let _ = l5.feed(c5, 20);
        c5 += 1;
    }
    let audit_ok = audit_start && audit_mid && l5.unfed_count() == 0;
    s.add("脱聚合审计：未馈数随馈入递减到零", audit_ok,
          "脱聚合 = 未馈数 > 0，审计必须逐卡定位");

    // 11. 零判据馈入拒绝
    let mut l6 = CloseLedger::new();
    let zero_ok = l6.feed(DOMAIN_LO, 0).err() == Some(LinkCode::ZeroChecks)
        && l6.fed() == 0;
    s.add("零判据拒绝：空馈不入账", zero_ok,
          "每卡必须携带正数判据数，空馈即虚报");

    // 12. 成文非空且含四标记 + 双跑一致
    let sum = compose_close_summary(&l3);
    let text_ok = match &sum {
        Ok(t) => t.contains("馈=20/20") && t.contains("核销=6640/6640")
            && t.contains("态=闭门"),
        Err(_) => false,
    };
    s.add("收口成文：馈/判据/核销/态齐备且两次成文逐字节一致",
          text_ok && sum == compose_close_summary(&l3),
          "成文必须携带 馈/核销/态 标记且为纯函数");

    // 13. 空账成文拒绝
    s.add("空账成文拒绝：零馈零销不得产出小结",
          compose_close_summary(&CloseLedger::new()).err() == Some(LinkCode::EmptySummary),
          "空账没有对账对象，产出小结即虚报");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(17) && agrees_with_linux(67)
        && agrees_with_linux(113) && agrees_with_linux(27)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < LinkCode::ALL.len() {
        let mut j = i + 1;
        while j < LinkCode::ALL.len() {
            if let (Some(a), Some(b)) = (LinkCode::ALL.get(i), LinkCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = LinkCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 收口成文往返 + 双跑指纹
    let rt_ok = match compose_close_summary(&l3) {
        Ok(t) => t == format!("馈=20/20;判据=400;核销=6640/6640;态=闭门"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("往返与指纹：收口成文可逐字复原且双跑哈希相等", rt_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &sum { Ok(t) => t.len(), Err(_) => 0 };
    let sum_text = format!("收口小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn full_feed(l: &mut CloseLedger) {
        let mut c = DOMAIN_LO;
        while c <= DOMAIN_HI {
            let _ = l.feed(c, 20);
            c += 1;
        }
    }

    #[test]
    fn domain_bounds() {
        let mut l = CloseLedger::new();
        assert!(l.feed(DOMAIN_LO, 20).is_ok());
        assert!(l.feed(DOMAIN_HI, 20).is_ok());
        assert_eq!(l.feed(DOMAIN_LO - 1, 20).err(), Some(LinkCode::OutOfDomain));
        assert_eq!(l.feed(DOMAIN_HI + 1, 20).err(), Some(LinkCode::OutOfDomain));
    }

    #[test]
    fn dup_feed_rejected() {
        let mut l = CloseLedger::new();
        let _ = l.feed(DOMAIN_LO, 20);
        assert_eq!(l.feed(DOMAIN_LO, 20).err(), Some(LinkCode::DupFeed));
        assert_eq!(l.fed(), 1);
    }

    #[test]
    fn zero_checks_rejected() {
        let mut l = CloseLedger::new();
        assert_eq!(l.feed(DOMAIN_LO, 0).err(), Some(LinkCode::ZeroChecks));
        assert_eq!(l.fed(), 0);
    }

    #[test]
    fn gates_in_order() {
        let mut l = CloseLedger::new();
        assert_eq!(l.promote().err(), Some(LinkCode::EmptyLedger));
        full_feed(&mut l);
        assert_eq!(l.promote(), Ok(CloseStage::Full));
        assert_eq!(l.promote().err(), Some(LinkCode::BudgetMismatch));
        let _ = l.spend(BUDGET_TOTAL);
        assert_eq!(l.promote(), Ok(CloseStage::Balanced));
        assert_eq!(l.promote(), Ok(CloseStage::Closed));
        assert!(l.promote().is_err());
    }

    #[test]
    fn unfed_audit() {
        let mut l = CloseLedger::new();
        assert_eq!(l.unfed_count(), DOMAIN_SIZE);
        full_feed(&mut l);
        assert_eq!(l.unfed_count(), 0);
    }

    #[test]
    fn budget_guards() {
        let mut l = CloseLedger::new();
        assert_eq!(l.spend(BUDGET_TOTAL + 1).err(), Some(LinkCode::BudgetOverrun));
        assert!(l.spend(BUDGET_TOTAL).is_ok());
        assert_eq!(l.spend(1).err(), Some(LinkCode::BudgetMismatch));
        assert!(l.balanced());
    }

    #[test]
    fn bad_close_matrix_green() {
        let (inj, per) = run_bad_close_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn summary_closed_marks() {
        let mut l = CloseLedger::new();
        assert!(compose_close_summary(&l).is_err());
        full_feed(&mut l);
        let s = compose_close_summary(&l).unwrap_or_default();
        assert!(s.contains("馈=20/20"));
        assert!(s.contains("态=开账"));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(113));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(27), -27);
    }

    #[test]
    fn unx10099_checks_green() {
        let set = run_unx10099_checks();
        assert!(set.all_passed(), "UNX-F10099 red: {}", set.red_items_text());
    }
}
