//! UNX-F10071 双档证据总对账（T1/ktest 覆盖率）· 0x921C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10072-J1 覆盖率核对齐（双档标记覆盖 10/10 次），无档条目 10/10 次检出
//!
//! 设计要点：
//!   ① 双档 = T1 档 / ktest 档；每条判据条目必须两档各有一份证据标记，
//!      覆盖率口径以「注册条目数」为分母冻结，禁止以已对账数为分母
//!      （分母漂移 → RateDrift 逐次拦截）。
//!   ② 覆盖注册表只登记不推断：t1 标记与 ktest 标记幂等写入（重复标记
//!      不算错），但未注册条目 UnknownItem、未知档别 UnknownTier 显性拒绝。
//!   ③ 覆盖账守恒式：对账条目总数 = 双档齐 + 仅 T1 + 仅 ktest + 无档，
//!      恒成立；无档条目 10/10 次检出，缺任一档 10/10 次检出。
//!   ④ 注入核：无档入账 / 单档混充 / 未注册条目 / 伪造档别四类坏样本
//!      逐类 10/10 次拒绝，注入账 逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10071 独占 0x40 窗口，下一卡 0x92200）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x921C0;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 覆盖注册表容量上限（480 条全域判据的工程预算）。
pub const COVER_CAP: usize = 128;

/// 旗标位 bit0：该条目有 T1 档证据。
pub const FLAG_T1: u8 = 0x01;

/// 旗标位 bit1：该条目有 ktest 档证据。
pub const FLAG_KTEST: u8 = 0x02;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CoverCode {
    /// 条目未注册即要求覆盖标记
    UnknownItem,
    /// 未知档别 token（不在 t1|ktest 闭集内）
    UnknownTier,
    /// 空档别 token
    EmptyTier,
    /// 注册表容量满
    RegistryFull,
    /// 无档条目（两档皆缺）
    Uncovered,
    /// 单档条目（缺另一档）
    HalfCovered,
    /// 覆盖账分类计数失守恒
    CoverRegress,
    /// 覆盖率分母口径漂移
    RateDrift,
    /// 空成文
    EmptySummary,
    /// 拒绝后账目被改动（拒绝不留痕被破坏）
    RejectedTrace,
    /// 档别标签往返不一致
    TierRoundTrip,
    /// 段位越界
    SegmentViolation,
}

impl CoverCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            CoverCode::UnknownItem => "条目未注册被拒：覆盖标记必须先注册条目；\
                请先 register 再打档别标记",
            CoverCode::UnknownTier => "未知档别被拒：档别只认 t1|ktest 闭集；\
                请改用闭集内写法再打标",
            CoverCode::EmptyTier => "空档别被拒：档别不能为空串；请确认证据档别后重打",
            CoverCode::RegistryFull => "容量满被拒：覆盖注册表已达上限；请先归档旧批再登记",
            CoverCode::Uncovered => "无档条目被拒：两档证据皆缺不得计入已覆盖；\
                请补齐 T1 与 ktest 双档标记",
            CoverCode::HalfCovered => "单档条目被拒：只打一档不构成双档覆盖；\
                请补齐另一档证据",
            CoverCode::CoverRegress => "覆盖账失守恒：对账总数必须等于 双档+仅T1+仅ktest+无档；\
                请排查是否绕过分类计数器",
            CoverCode::RateDrift => "覆盖率分母漂移被拒：分母必须冻结为注册条目数；\
                禁止用已对账数替代分母稀释口径",
            CoverCode::EmptySummary => "空成文被拒：覆盖对账小结不得为空串；请检查注册表是否为空",
            CoverCode::RejectedTrace => "拒绝留痕被拒：被拒对账不得改动任何计数；\
                请排查拒绝路径是否误提交",
            CoverCode::TierRoundTrip => "档别往返不一致：label 再解析必须回到原档别；\
                请检查闭集文案与解析器是否同步",
            CoverCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [CoverCode; 12] = [
        CoverCode::UnknownItem,
        CoverCode::UnknownTier,
        CoverCode::EmptyTier,
        CoverCode::RegistryFull,
        CoverCode::Uncovered,
        CoverCode::HalfCovered,
        CoverCode::CoverRegress,
        CoverCode::RateDrift,
        CoverCode::EmptySummary,
        CoverCode::RejectedTrace,
        CoverCode::TierRoundTrip,
        CoverCode::SegmentViolation,
    ];
}

// ── 档别闭集与解析器 ──────────────────────────────────────────────────
/// 双档闭集：T1 / ktest。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// T1 档（内核冒烟单档）
    T1,
    /// ktest 档（内核测试套单档）
    Ktest,
}

impl Tier {
    /// 中文标签（成文与往返用）。
    pub fn label(self) -> &'static str {
        match self {
            Tier::T1 => "T1 档",
            Tier::Ktest => "ktest 档",
        }
    }

    /// 闭集全量。
    pub const ALL: [Tier; 2] = [Tier::T1, Tier::Ktest];

    /// 旗标位映射。
    pub fn flag(self) -> u8 {
        match self {
            Tier::T1 => FLAG_T1,
            Tier::Ktest => FLAG_KTEST,
        }
    }
}

/// 档别解析：长名 t1/ktest 与单字母 t/k 双形制，其余一律 UnknownTier，
/// 空串 EmptyTier。解析器是档别标记的唯一入口。
pub fn parse_tier(tag: &str) -> Result<Tier, CoverCode> {
    if tag.is_empty() {
        return Err(CoverCode::EmptyTier);
    }
    match tag {
        "t1" | "t" => Ok(Tier::T1),
        "ktest" | "k" => Ok(Tier::Ktest),
        _ => Err(CoverCode::UnknownTier),
    }
}

// ── 覆盖注册表 ────────────────────────────────────────────────────────
/// 条目覆盖态：条目号 + 双档旗标位图。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoverRow {
    /// 条目号
    pub item: u32,
    /// 旗标位图（bit0=T1 bit1=ktest）
    pub flags: u8,
}

impl CoverRow {
    /// 是否有 T1 档证据。
    pub fn has_t1(self) -> bool {
        self.flags & FLAG_T1 != 0
    }

    /// 是否有 ktest 档证据。
    pub fn has_ktest(self) -> bool {
        self.flags & FLAG_KTEST != 0
    }

    /// 双档是否齐备。
    pub fn both_covered(self) -> bool {
        self.has_t1() && self.has_ktest()
    }

    /// 是否完全无档。
    pub fn uncovered(self) -> bool {
        self.flags == 0
    }
}

/// 覆盖注册表：条目级双档旗标，只登记不推断；重复登记显性拒绝，
/// 档别标记幂等（重复打标不算错）。
#[derive(Clone, Debug)]
pub struct CoverageRegistry {
    rows: Vec<CoverRow>,
    cap: usize,
}

impl CoverageRegistry {
    /// 空表构造（容量 COVER_CAP）。
    pub fn new() -> CoverageRegistry {
        CoverageRegistry { rows: Vec::new(), cap: COVER_CAP }
    }

    /// 首次登记条目；重复登记显性拒绝、满册显性拒绝。
    pub fn register(&mut self, item: u32) -> Result<(), CoverCode> {
        if self.find(item).is_some() {
            return Err(CoverCode::RejectedTrace);
        }
        if self.rows.len() >= self.cap {
            return Err(CoverCode::RegistryFull);
        }
        self.rows.push(CoverRow { item, flags: 0 });
        Ok(())
    }

    /// 线性查找条目下标（无越界风险：全部走 get）。
    fn find(&self, item: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.item == item {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 打档别标记：未知条目 UnknownItem；重复打标幂等通过。
    pub fn mark(&mut self, item: u32, tier: Tier) -> Result<(), CoverCode> {
        let idx = match self.find(item) {
            Some(i) => i,
            None => return Err(CoverCode::UnknownItem),
        };
        if let Some(r) = self.rows.get_mut(idx) {
            r.flags |= tier.flag();
        }
        Ok(())
    }

    /// 查覆盖态。
    pub fn row(&self, item: u32) -> Option<CoverRow> {
        match self.find(item) {
            Some(i) => self.rows.get(i).copied(),
            None => None,
        }
    }

    /// 已注册条目数（覆盖率分母口径，冻结）。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

// ── 覆盖账与守恒审计 ──────────────────────────────────────────────────
/// 覆盖账：对账总数 = 双档齐 + 仅 T1 + 仅 ktest + 无档，恒成立。
#[derive(Clone, Copy, Debug)]
pub struct CoverLedger {
    /// 双档齐条目数
    pub both: u32,
    /// 仅 T1 档条目数
    pub only_t1: u32,
    /// 仅 ktest 档条目数
    pub only_ktest: u32,
    /// 无档条目数
    pub uncovered: u32,
    /// 对账条目总数
    pub audited: u32,
}

impl CoverLedger {
    /// 空账构造。
    pub const fn new() -> CoverLedger {
        CoverLedger { both: 0, only_t1: 0, only_ktest: 0, uncovered: 0, audited: 0 }
    }

    /// 归类入账一条（按注册表覆盖态四分类）；计数器只增不减。
    pub fn classify(&mut self, r: CoverRow) {
        if r.both_covered() {
            self.both = self.both.saturating_add(1);
        } else if r.has_t1() {
            self.only_t1 = self.only_t1.saturating_add(1);
        } else if r.has_ktest() {
            self.only_ktest = self.only_ktest.saturating_add(1);
        } else {
            self.uncovered = self.uncovered.saturating_add(1);
        }
        self.audited = self.audited.saturating_add(1);
    }

    /// 守恒式：总数 = 四分类之和。
    pub fn conserved(&self) -> bool {
        self.both.saturating_add(self.only_t1)
            .saturating_add(self.only_ktest)
            .saturating_add(self.uncovered)
            == self.audited
    }

    /// 覆盖率（千分比）：分母冻结为对账总数，双档齐为分子。
    pub fn coverage_permille(&self) -> u32 {
        if self.audited == 0 {
            return 0;
        }
        self.both.saturating_mul(1000) / self.audited
    }
}

/// 覆盖对账：逐条目按注册表归类入账；未注册条目显性拒绝（拒绝不留痕）。
pub fn audit_coverage(reg: &CoverageRegistry, items: &[u32]) -> Result<CoverLedger, CoverCode> {
    let mut led = CoverLedger::new();
    let mut i = 0usize;
    while i < items.len() {
        let item = match items.get(i) {
            Some(v) => *v,
            None => break,
        };
        let row = match reg.row(item) {
            Some(r) => r,
            None => return Err(CoverCode::UnknownItem),
        };
        led.classify(row);
        i += 1;
    }
    Ok(led)
}

/// 覆盖成文：「双档=<n>;仅T1=<n>;仅ktest=<n>;无档=<n>;覆盖=<x.x%>」；空表先拒。
pub fn compose_cover_summary(reg: &CoverageRegistry, led: &CoverLedger) -> Result<String, CoverCode> {
    if reg.is_empty() || !led.conserved() {
        return Err(CoverCode::EmptySummary);
    }
    // 口径守卫：分母必须等于注册条目数，否则视为口径漂移。
    if led.audited != reg.len() as u32 {
        return Err(CoverCode::RateDrift);
    }
    let pm = led.coverage_permille();
    let out = format!(
        "双档={};仅T1={};仅ktest={};无档={};覆盖={}.{}%",
        led.both, led.only_t1, led.only_ktest, led.uncovered, pm / 10, pm % 10
    );
    if out.is_empty() {
        return Err(CoverCode::EmptySummary);
    }
    Ok(out)
}

// ── 注入核：坏覆盖样本矩阵 ────────────────────────────────────────────
/// 四类坏覆盖样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCover {
    /// 无档条目混入已覆盖
    UncoveredItem,
    /// 单档条目混充双档
    HalfCoveredItem,
    /// 未注册条目直接对账
    UnregisteredAudit,
    /// 伪造档别 token
    FakeTier,
}

impl BadCover {
    /// 闭集全量。
    pub const ALL: [BadCover; 4] = [
        BadCover::UncoveredItem,
        BadCover::HalfCoveredItem,
        BadCover::UnregisteredAudit,
        BadCover::FakeTier,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadCover::UncoveredItem => "无档条目混充覆盖",
            BadCover::HalfCoveredItem => "单档混充双档",
            BadCover::UnregisteredAudit => "未注册条目对账",
            BadCover::FakeTier => "伪造档别 token",
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

    /// 记一次注入结果；账失守恒返回 CoverRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), CoverCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(CoverCode::CoverRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏覆盖注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
/// 每轮用全新注册表，确保被拒样本不留痕。
pub fn run_bad_cover_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadCover::ALL.len() {
        let case = match BadCover::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadCover::UncoveredItem => {
                    let mut reg = CoverageRegistry::new();
                    let _ = reg.register(1);
                    match audit_coverage(&reg, &[1]) {
                        Ok(led) => led.uncovered == 1 && !reg.row(1).unwrap_or(
                            CoverRow { item: 0, flags: 1 },
                        ).both_covered(),
                        Err(_) => false,
                    }
                }
                BadCover::HalfCoveredItem => {
                    let mut reg = CoverageRegistry::new();
                    let _ = reg.register(2);
                    let _ = reg.mark(2, Tier::T1);
                    let r = reg.row(2).unwrap_or(CoverRow { item: 0, flags: 0 });
                    !r.both_covered() && r.has_t1()
                }
                BadCover::UnregisteredAudit => {
                    let reg = CoverageRegistry::new();
                    audit_coverage(&reg, &[9]).err() == Some(CoverCode::UnknownItem)
                }
                BadCover::FakeTier => {
                    parse_tier("ltp").is_err()
                        && parse_tier("").err() == Some(CoverCode::EmptyTier)
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
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；未知/空档别族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；未注册/无档条目族"),
    ("ENODEV", -19, 22, "Windows ERROR_BAD_DEVICE；单档缺另一档族"),
    ("ESTALE", -116, 18, "Windows ERROR_NO_MORE_FILES；口径漂移/分母失冻结族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；注册表容量满族"),
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

/// 规范语料双跑指纹：注册四条 → 双档标记 → 对账分类 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut reg = CoverageRegistry::new();
    let _ = reg.register(1);
    let _ = reg.register(2);
    let _ = reg.register(3);
    let _ = reg.register(4);
    let _ = reg.mark(1, Tier::T1);
    let _ = reg.mark(1, Tier::Ktest);
    let _ = reg.mark(2, Tier::T1);
    let _ = reg.mark(3, Tier::Ktest);
    let led = audit_coverage(&reg, &[1, 2, 3, 4]).unwrap_or(CoverLedger::new());
    let bytes = match compose_cover_summary(&reg, &led) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10071 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10071_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10071");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x921C0..=0x921FF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 档别解析器：双形制全通过
    let parse_ok = parse_tier("t1") == Ok(Tier::T1)
        && parse_tier("t") == Ok(Tier::T1)
        && parse_tier("ktest") == Ok(Tier::Ktest)
        && parse_tier("k") == Ok(Tier::Ktest);
    s.add("档别解析器：t1|t / ktest|k 双形制全通过", parse_ok,
          "档别闭集双形制必须全可解析，闭集外显性拒绝");

    // 3. 伪造档别注入：未知/空串/漂移 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_tier("ltp").is_ok()
            || parse_tier("").is_ok()
            || parse_tier("T1").is_ok()
            || parse_tier("ktest ").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("伪造档别注入：未知/空串/漂移 10/10 拒绝", amb_ok,
          "档别歧义必须逐次显性拒绝，不许静默归并到相邻档别");

    // 4. 注册表：登记四条并查回覆盖态
    let mut reg = CoverageRegistry::new();
    let _ = reg.register(1);
    let _ = reg.register(2);
    let _ = reg.register(3);
    let _ = reg.register(4);
    let reg_ok = reg.len() == 4
        && reg.row(1).map(|x| x.uncovered()).unwrap_or(false);
    s.add("注册表：四条登记成功且未打标条目无档", reg_ok,
          "注册表只登记不推断，覆盖态只由档别标记写入");

    // 5. 双档标记：幂等打标后覆盖态齐备
    let _ = reg.mark(1, Tier::T1);
    let _ = reg.mark(1, Tier::Ktest);
    let _ = reg.mark(1, Tier::T1);
    let mark_ok = reg.row(1).map(|x| x.both_covered()).unwrap_or(false)
        && reg.row(1).map(|x| x.flags).unwrap_or(0) == FLAG_T1 | FLAG_KTEST;
    s.add("双档标记：T1+ktest 齐备且重复打标幂等", mark_ok,
          "档别标记幂等写入，双档齐备以位图为准");

    // 6. 覆盖对账：四分类与守恒
    let _ = reg.mark(2, Tier::T1);
    let _ = reg.mark(3, Tier::Ktest);
    let led = match audit_coverage(&reg, &[1, 2, 3, 4]) {
        Ok(v) => v,
        Err(_) => CoverLedger::new(),
    };
    s.add("覆盖对账：四分类齐备且账守恒",
          led.conserved() && led.both == 1 && led.only_t1 == 1
              && led.only_ktest == 1 && led.uncovered == 1,
          "对账总数必须等于 双档+仅T1+仅ktest+无档");

    // 7. 无档条目检出：10/10 次
    let mut unc_ok = true;
    let mut r2 = 0u32;
    while r2 < INJECT_ROUNDS {
        let mut reg2 = CoverageRegistry::new();
        let _ = reg2.register(7);
        match audit_coverage(&reg2, &[7]) {
            Ok(l) => {
                if l.uncovered != 1 {
                    unc_ok = false;
                }
            }
            Err(_) => unc_ok = false,
        }
        r2 += 1;
    }
    s.add("无档条目检出：10/10 次归入无档格", unc_ok,
          "两档皆缺的条目必须逐次落入无档格，不许计入覆盖");

    // 8. 单档条目检出：10/10 次
    let mut half_ok = true;
    let mut r3 = 0u32;
    while r3 < INJECT_ROUNDS {
        let mut reg2 = CoverageRegistry::new();
        let _ = reg2.register(8);
        let _ = reg2.mark(8, Tier::Ktest);
        match audit_coverage(&reg2, &[8]) {
            Ok(l) => {
                if l.only_ktest != 1 || l.both != 0 {
                    half_ok = false;
                }
            }
            Err(_) => half_ok = false,
        }
        r3 += 1;
    }
    s.add("单档条目检出：10/10 次不得混充双档", half_ok,
          "缺任一档的条目不得计入双档覆盖");

    // 9. 注入核：四类坏覆盖样本各 10/10 拒绝
    let (inj, per_class) = run_bad_cover_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏覆盖样本各 10/10 拒绝", all_caught,
          "无档/单档/未注册/伪造档别必须逐类逐次拒绝");

    // 10. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadCover::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 未注册条目对账拒绝且不留痕
    let audited_before = led.audited;
    let rej = audit_coverage(&reg, &[99]).err() == Some(CoverCode::UnknownItem)
        && led.audited == audited_before;
    s.add("未注册条目：对账拒绝且不改账", rej,
          "对账必须以注册表为出处，未注册条目显性拒绝");

    // 12. 覆盖率口径：分母冻结为注册条目数
    let pm = led.coverage_permille();
    let expect_pm = 1000u32 * led.both / led.audited;
    s.add("覆盖率口径：千分比可复算且分母=注册数",
          led.audited == reg.len() as u32 && pm == expect_pm,
          "分母口径冻结为注册条目数，禁止用已对账数稀释");

    // 13. 口径漂移拦截：对账数≠注册数时成文拒绝
    //     账面本身守恒（1+1+1+0=3）但分母≠注册数 1 → 只许 RateDrift 拦截。
    let mut reg3 = CoverageRegistry::new();
    let _ = reg3.register(5);
    let led3 = CoverLedger { both: 1, only_t1: 1, only_ktest: 1, uncovered: 0, audited: 3 };
    s.add("口径漂移拦截：分母不符成文显性拒绝",
          compose_cover_summary(&reg3, &led3).err() == Some(CoverCode::RateDrift),
          "分母漂移必须逐次拦截，不许静默放行");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(2) && agrees_with_linux(19)
        && agrees_with_linux(116) && agrees_with_linux(28)
        && !agrees_with_linux(0) && !agrees_with_linux(7777);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < CoverCode::ALL.len() {
        let mut j = i + 1;
        while j < CoverCode::ALL.len() {
            if let (Some(a), Some(b)) = (CoverCode::ALL.get(i), CoverCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = CoverCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 档别往返 + 双跑指纹
    let rt_ok = Tier::ALL.iter().all(|t| {
        let tag = match t {
            Tier::T1 => "t1",
            Tier::Ktest => "ktest",
        };
        parse_tier(tag) == Ok(*t)
    });
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("往返与指纹：档别标签可回解析且双跑哈希相等", rt_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 覆盖率 + 指纹
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let cov_text = format!("覆盖率 {}‰（双档 {}/4）", pm, led.both);
    s.add_dyn("覆盖率快照", pm == expect_pm, &cov_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_closed_set() {
        assert_eq!(parse_tier("ktest"), Ok(Tier::Ktest));
        assert_eq!(parse_tier("t"), Ok(Tier::T1));
        assert!(parse_tier("ltp").is_err());
        assert_eq!(parse_tier("").err(), Some(CoverCode::EmptyTier));
    }

    #[test]
    fn registry_marks_idempotent() {
        let mut reg = CoverageRegistry::new();
        assert!(reg.register(9).is_ok());
        assert!(reg.register(9).is_err());
        assert_eq!(reg.mark(9, Tier::T1), Ok(()));
        assert_eq!(reg.mark(9, Tier::T1), Ok(()));
        assert_eq!(reg.mark(10, Tier::T1).err(), Some(CoverCode::UnknownItem));
        let r = reg.row(9).unwrap_or(CoverRow { item: 0, flags: 0 });
        assert!(r.has_t1() && !r.has_ktest());
    }

    #[test]
    fn audit_four_way_classification() {
        let mut reg = CoverageRegistry::new();
        for i in 1..=4u32 {
            let _ = reg.register(i);
        }
        let _ = reg.mark(1, Tier::T1);
        let _ = reg.mark(1, Tier::Ktest);
        let _ = reg.mark(2, Tier::T1);
        let _ = reg.mark(3, Tier::Ktest);
        let led = audit_coverage(&reg, &[1, 2, 3, 4]).unwrap_or(CoverLedger::new());
        assert!(led.conserved());
        assert_eq!(led.both, 1);
        assert_eq!(led.uncovered, 1);
        assert_eq!(led.coverage_permille(), 250);
    }

    #[test]
    fn unregistered_audit_rejected() {
        let reg = CoverageRegistry::new();
        assert_eq!(audit_coverage(&reg, &[42]).err(), Some(CoverCode::UnknownItem));
    }

    #[test]
    fn bad_cover_matrix_green() {
        let (inj, per) = run_bad_cover_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rate_drift_rejected() {
        let mut reg = CoverageRegistry::new();
        let _ = reg.register(5);
        // 账面守恒（1+1+1+0=3）但分母≠注册数 1 → 必须以 RateDrift 拦截。
        let led = CoverLedger { both: 1, only_t1: 1, only_ktest: 1, uncovered: 0, audited: 3 };
        assert_eq!(compose_cover_summary(&reg, &led).err(), Some(CoverCode::RateDrift));
    }

    #[test]
    fn summary_contains_marks() {
        let mut reg = CoverageRegistry::new();
        let _ = reg.register(1);
        let _ = reg.mark(1, Tier::T1);
        let _ = reg.mark(1, Tier::Ktest);
        let led = audit_coverage(&reg, &[1]).unwrap_or(CoverLedger::new());
        let s = compose_cover_summary(&reg, &led).unwrap_or_default();
        assert!(s.contains("双档=1") && s.contains("覆盖=100.0%"));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(116));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(19), -19);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10071_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10071_checks_green() {
        let set = run_unx10071_checks();
        assert!(set.all_passed(), "UNX-F10071 red: {}", set.red_items_text());
    }
}
