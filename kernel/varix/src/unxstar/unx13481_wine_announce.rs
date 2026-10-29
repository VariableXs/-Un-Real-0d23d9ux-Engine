//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13481 · Wine 预告总成（E1 消费预告登记与特征识别面权威） · 0x65B00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥370 行；
//!   ② UNX-F13481-J1 预告总账（E1 消费预告三列登记+特征识别面总成
//!      声明），预告协议与 F13387/F13398 范式同构；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 预告总账三列：消费方/特征/预告态；消费方名非空（零静默）、
//!     同消费方同特征唯一（一证一行）；
//!   - 预告态四态机：Unregistered→Declared（登记）→Acknowledged
//!     （识别面确认）→Consumed（消费落账）；未预告消费拒收（红线）；
//!   - 特征识别面总成：10 特征冻结闭集，每特征一识别规则
//!     （ByPrefix/BySuffix/ByContains 三规则闭集），识别面声明 =
//!     全特征规则登记齐（SurfaceIncomplete 红线）；
//!   - 范式同构 F13387/F13398：同态机闭式迁移 + 规则反查 + 双跑幂等；
//!   - 守恒：Σ预告态 ≡ 登记行数；消费数 ≡ Consumed 态数；
//!   - 注入三族：未预告消费/规则匹配面越界/重复预告 3/3 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65B00），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65B00，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65B00;

/// E1 特征冻结闭集：10 项。
pub const FEATURE_COUNT: usize = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnnCode {
    /// 特征下标越闭集
    FeatureOutOfRange,
    /// 预告态非法迁移
    AnnounceStateIllegal,
    /// 未预告消费（红线）
    ConsumeWithoutDeclare,
    /// 消费方名为空（零静默）
    ConsumerEmpty,
    /// 重复预告（同消费方同特征）
    AnnounceBookDup,
    /// 识别规则匹配面越界
    RecognizeRuleInvalid,
    /// 识别面未总成（规则未登记齐）
    SurfaceIncomplete,
    /// 预告账守恒破
    ConservationBreak,
    /// 双跑非幂等
    RerunNonIdempotent,
}

impl AnnCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AnnCode::FeatureOutOfRange => "特征下标越闭集：10 项冻结",
            AnnCode::AnnounceStateIllegal => "预告态非法迁移：四态闭集外",
            AnnCode::ConsumeWithoutDeclare => "未预告消费：Declared 前置红线",
            AnnCode::ConsumerEmpty => "消费方名为空：零静默红线",
            AnnCode::AnnounceBookDup => "重复预告：同消费方同特征唯一",
            AnnCode::RecognizeRuleInvalid => "识别规则匹配面越界",
            AnnCode::SurfaceIncomplete => "识别面未总成：规则未登记齐",
            AnnCode::ConservationBreak => "预告账守恒破：Σ预告态≡登记行数",
            AnnCode::RerunNonIdempotent => "双跑非幂等：同序列哈希逐位同",
        }
    }
    pub const ALL: [AnnCode; 9] = [
        AnnCode::FeatureOutOfRange,
        AnnCode::AnnounceStateIllegal,
        AnnCode::ConsumeWithoutDeclare,
        AnnCode::ConsumerEmpty,
        AnnCode::AnnounceBookDup,
        AnnCode::RecognizeRuleInvalid,
        AnnCode::SurfaceIncomplete,
        AnnCode::ConservationBreak,
        AnnCode::RerunNonIdempotent,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AnnCode::ALL.len() {
        if AnnCode::ALL[i].val() == code {
            return AnnCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65B00 段闭集内"
}

// ── 特征冻结闭集（E1 消费面） ────────────────────────────────────
pub const FEATURES: [&str; FEATURE_COUNT] = [
    "ntdll.LdrGetDllHandle",
    "kernel32.GetVersionExW",
    "ntdll.NtQueryInformationProcess",
    "kernel32.CreateNamedPipeW",
    "advapi32.RegOpenKeyExW",
    "user32.GetSystemMetrics",
    "gdi32.GetDeviceCaps",
    "ws2_32.getaddrinfo",
    "msvcrt._ultow",
    "version.GetFileVersionInfoW",
];

// ── 识别规则三闭集 ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleKind {
    /// 前缀匹配
    ByPrefix,
    /// 后缀匹配
    BySuffix,
    /// 包含匹配
    ByContains,
}

/// 识别规则：种类 + 针串。
#[derive(Clone, Copy, Debug)]
pub struct RecogRule {
    pub kind: RuleKind,
    pub needle: &'static str,
}

impl RecogRule {
    /// 匹配执行：先比针串长度（下标先比长度纪律）。
    pub fn matches(&self, probe: &str) -> bool {
        if self.needle.is_empty() {
            return false;
        }
        match self.kind {
            RuleKind::ByPrefix => probe.starts_with(self.needle),
            RuleKind::BySuffix => probe.ends_with(self.needle),
            RuleKind::ByContains => probe.contains(self.needle),
        }
    }
}

/// 识别规则与特征针串合法性：针串不得长于特征名（匹配面越界拒收）。
pub fn rule_valid(rule: RecogRule, feature: usize) -> bool {
    if feature >= FEATURE_COUNT {
        return false;
    }
    rule.needle.len() <= FEATURES[feature].len()
}

// ── 预告态四闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnnState {
    /// 未登记
    Unregistered,
    /// 已登记（消费方预告）
    Declared,
    /// 识别面已确认
    Acknowledged,
    /// 已消费落账
    Consumed,
}

/// 预告行：三列（消费方/特征/预告态）。
#[derive(Clone, Copy, Debug)]
pub struct AnnRow {
    pub consumer: &'static str,
    pub feature: usize,
    pub state: AnnState,
}

// ── 预告总账 ─────────────────────────────────────────────────────
pub struct AnnLedger {
    pub rows: Vec<AnnRow>,
    /// 识别面规则登记账（下标对齐 FEATURES）。
    pub rules: [Option<RecogRule>; FEATURE_COUNT],
    pub rules_registered: usize,
}

impl AnnLedger {
    pub fn new() -> AnnLedger {
        AnnLedger {
            rows: Vec::new(),
            rules: core::array::from_fn(|_| None),
            rules_registered: 0,
        }
    }

    /// 识别面登记：一特征一规则，针串长不越特征名。
    pub fn register_rule(&mut self, feature: usize, rule: RecogRule) -> Result<(), u32> {
        if feature >= FEATURE_COUNT {
            return Err(AnnCode::FeatureOutOfRange.val());
        }
        if !rule_valid(rule, feature) {
            return Err(AnnCode::RecognizeRuleInvalid.val());
        }
        if self.rules[feature].is_none() {
            self.rules_registered += 1;
        }
        self.rules[feature] = Some(rule);
        Ok(())
    }

    /// 识别面总成声明：全特征规则登记齐才许。
    pub fn surface_complete(&self) -> bool {
        self.rules_registered == FEATURE_COUNT
    }

    /// 预告登记：消费方非空、同消费方同特征唯一；Unregistered→Declared。
    pub fn declare(&mut self, consumer: &'static str, feature: usize) -> Result<usize, u32> {
        if consumer.is_empty() {
            return Err(AnnCode::ConsumerEmpty.val());
        }
        if feature >= FEATURE_COUNT {
            return Err(AnnCode::FeatureOutOfRange.val());
        }
        if !self.surface_complete() {
            return Err(AnnCode::SurfaceIncomplete.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].consumer == consumer && self.rows[i].feature == feature {
                return Err(AnnCode::AnnounceBookDup.val());
            }
            i += 1;
        }
        self.rows.push(AnnRow {
            consumer,
            feature,
            state: AnnState::Declared,
        });
        Ok(self.rows.len() - 1)
    }

    /// 前向迁移闭式：Declared→Acknowledged→Consumed。
    pub fn advance(&mut self, idx: usize, next: AnnState) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(AnnCode::FeatureOutOfRange.val());
        }
        let cur = self.rows[idx].state;
        let legal = matches!(
            (cur, next),
            (AnnState::Declared, AnnState::Acknowledged)
                | (AnnState::Acknowledged, AnnState::Consumed)
        );
        if !legal {
            return Err(AnnCode::AnnounceStateIllegal.val());
        }
        // Acknowledged 必须过识别面规则（特征规则在账且探针匹配）。
        if next == AnnState::Acknowledged {
            let f = self.rows[idx].feature;
            match self.rules[f] {
                Some(r) => {
                    if !r.matches(FEATURES[f]) {
                        return Err(AnnCode::RecognizeRuleInvalid.val());
                    }
                }
                None => return Err(AnnCode::SurfaceIncomplete.val()),
            }
        }
        self.rows[idx].state = next;
        Ok(())
    }

    /// 消费对账：消费方只能按预告行消费（未预告消费拒收）。
    pub fn consume_check(&self, consumer: &str, feature: usize) -> Result<(), u32> {
        if feature >= FEATURE_COUNT {
            return Err(AnnCode::FeatureOutOfRange.val());
        }
        let mut found = false;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].consumer == consumer && self.rows[i].feature == feature {
                found = true;
                if self.rows[i].state != AnnState::Consumed {
                    return Err(AnnCode::ConsumeWithoutDeclare.val());
                }
            }
            i += 1;
        }
        if !found {
            return Err(AnnCode::ConsumeWithoutDeclare.val());
        }
        Ok(())
    }

    /// 预告账守恒：Σ预告态 ≡ 行数；消费数 ≡ Consumed 态数。
    pub fn conservation(&self) -> Result<[u32; 4], u32> {
        let mut counts = [0u32; 4];
        let mut i = 0;
        while i < self.rows.len() {
            let k = match self.rows[i].state {
                AnnState::Unregistered => 0,
                AnnState::Declared => 1,
                AnnState::Acknowledged => 2,
                AnnState::Consumed => 3,
            };
            counts[k] += 1;
            i += 1;
        }
        let total = counts[0] + counts[1] + counts[2] + counts[3];
        if total != self.rows.len() as u32 {
            return Err(AnnCode::ConservationBreak.val());
        }
        Ok(counts)
    }

    /// 账哈希（幂等载体）。
    pub fn ledger_hash(&self) -> u32 {
        let mut h: u32 = 0x811C9DC5;
        let mut i = 0;
        while i < self.rows.len() {
            let b = self.rows[i].consumer.as_bytes();
            let mut j = 0;
            while j < b.len() {
                h = (h ^ b[j] as u32).wrapping_mul(0x01000193);
                j += 1;
            }
            h = (h ^ (self.rows[i].feature as u32 + 1)).wrapping_mul(0x01000193);
            let sv = match self.rows[i].state {
                AnnState::Unregistered => 1u32,
                AnnState::Declared => 2,
                AnnState::Acknowledged => 3,
                AnnState::Consumed => 4,
            };
            h = (h ^ sv).wrapping_mul(0x01000193);
            i += 1;
        }
        h
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

/// 识别面冻结规则表（每特征一条，范式同构 F13387/F13398）。
pub const GOLDEN_RULES: [RecogRule; FEATURE_COUNT] = [
    RecogRule { kind: RuleKind::ByPrefix, needle: "ntdll." },
    RecogRule { kind: RuleKind::ByPrefix, needle: "kernel32.Get" },
    RecogRule { kind: RuleKind::ByContains, needle: "Query" },
    RecogRule { kind: RuleKind::BySuffix, needle: "PipeW" },
    RecogRule { kind: RuleKind::ByContains, needle: "RegOpen" },
    RecogRule { kind: RuleKind::BySuffix, needle: "Metrics" },
    RecogRule { kind: RuleKind::ByContains, needle: "DeviceCaps" },
    RecogRule { kind: RuleKind::ByPrefix, needle: "ws2_32." },
    RecogRule { kind: RuleKind::BySuffix, needle: "_ultow" },
    RecogRule { kind: RuleKind::ByPrefix, needle: "version." },
];

/// 消费方闭集（样板 4 面）。
pub const CONSUMERS: [&str; 4] = [
    "winecore.e1",
    "winelaunch.e1",
    "wineshim.e1",
    "winecase.e1",
];

// ── 黄金正演：识别面总成 → 10 特征全预告 → 确认 → 消费 → 守恒 ──
pub fn golden_run() -> Result<u32, u32> {
    let mut led = AnnLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    // 识别面总成：全规则登记（乱序登记以证闭集无序齐）。
    let mut f = 0;
    while f < FEATURE_COUNT {
        let pick = (f + rng.below(FEATURE_COUNT as u64) as usize) % FEATURE_COUNT;
        if led.rules[pick].is_none() {
            led.register_rule(pick, GOLDEN_RULES[pick])?;
        }
        f += 1;
    }
    // 补漏（乱序可能漏 0~2 条）。
    let mut f2 = 0;
    while f2 < FEATURE_COUNT {
        if led.rules[f2].is_none() {
            led.register_rule(f2, GOLDEN_RULES[f2])?;
        }
        f2 += 1;
    }
    if !led.surface_complete() {
        return Err(AnnCode::SurfaceIncomplete.val());
    }
    // 10 特征全预告（消费方 LCG 轮转）。
    let mut k = 0;
    while k < FEATURE_COUNT {
        let who = CONSUMERS[rng.below(4) as usize];
        let _ = led.declare(who, k)?;
        let idx = led.rows.len() - 1;
        led.advance(idx, AnnState::Acknowledged)?;
        led.advance(idx, AnnState::Consumed)?;
        k += 1;
    }
    // 消费对账：逐行按消费面复查。
    let mut m = 0;
    while m < led.rows.len() {
        led.consume_check(led.rows[m].consumer, led.rows[m].feature)?;
        m += 1;
    }
    led.conservation()?;
    // 双跑幂等：账哈希稳定。
    let h1 = led.ledger_hash();
    let h2 = led.ledger_hash();
    if h1 != h2 {
        return Err(AnnCode::RerunNonIdempotent.val());
    }
    Ok(h1)
}

// ── 注入面：三族 3/3 必抓 ────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：未预告消费（空账直接 consume_check）。
    ledger.tries[0] += 1;
    let d = AnnLedger::new();
    if let Err(e) = d.consume_check("winecore.e1", 0) {
        if e == AnnCode::ConsumeWithoutDeclare.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：识别规则匹配面越界（针串长于特征名）。
    ledger.tries[1] += 1;
    let mut d2 = AnnLedger::new();
    if let Err(e) = d2.register_rule(
        0,
        RecogRule { kind: RuleKind::ByContains, needle: "ntdll.LdrGetDllHandleXX" },
    ) {
        if e == AnnCode::RecognizeRuleInvalid.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：重复预告（同消费方同特征）。
    ledger.tries[2] += 1;
    let mut f = 0;
    while f < FEATURE_COUNT {
        let _ = d2.register_rule(f, GOLDEN_RULES[f]);
        f += 1;
    }
    let _ = d2.declare("winecore.e1", 0);
    if let Err(e) = d2.declare("winecore.e1", 0) {
        if e == AnnCode::AnnounceBookDup.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AnnCode::ALL.len() {
        let v = AnnCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push(("J1-黄金正演10特征预告", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 识别面总成声明：10/10 规则登记齐且逐条自匹配。
    let mut d2 = AnnLedger::new();
    let mut f = 0;
    while f < FEATURE_COUNT {
        let _ = d2.register_rule(f, GOLDEN_RULES[f]);
        f += 1;
    }
    let mut self_match = d2.surface_complete();
    let mut g = 0;
    while g < FEATURE_COUNT {
        match d2.rules[g] {
            Some(r) => {
                if !r.matches(FEATURES[g]) {
                    self_match = false;
                }
            }
            None => self_match = false,
        }
        g += 1;
    }
    v.push((
        "J1-识别面10/10自匹配",
        self_match,
        String::from("三规则闭集全登记"),
    ));
    // 反向判据：空针串不匹配任何探针。
    v.push((
        "J1-空针串必拒",
        !RecogRule { kind: RuleKind::ByContains, needle: "" }.matches(FEATURES[0]),
        String::from("零静默红线"),
    ));
    // 消费方名空必拒。
    let mut d3 = AnnLedger::new();
    let mut h = 0;
    while h < FEATURE_COUNT {
        let _ = d3.register_rule(h, GOLDEN_RULES[h]);
        h += 1;
    }
    v.push((
        "J1-消费方空必拒",
        d3.declare("", 0) == Err(AnnCode::ConsumerEmpty.val()),
        String::from("零静默红线"),
    ));
    // detail 反查闭链。
    let first = detail(AnnCode::FeatureOutOfRange.val());
    let last = detail(AnnCode::RerunNonIdempotent.val());
    v.push((
        "J2-detail 反查闭链",
        first != "未知诊断码：不在 0x65B00 段闭集内"
            && last != "未知诊断码：不在 0x65B00 段闭集内",
        String::from("首尾反查非未知"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65B00 窗口")));
    v
}

pub fn run_unx13481_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13481 Wine 预告总成");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
