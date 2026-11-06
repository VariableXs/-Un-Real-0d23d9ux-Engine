//! UNX-F50087 字体替换表（缺失字体→替代映射显式登记）（0x43700 段）
//!
//! 锚点：UNX-F50087-J1 替换 ×20 显式记账零静默替换；判据锚定 体验九 错误三要素。
//!
//! 数据段 `SEG_BASE = 0x43700`：诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 10`，
//! 逐项见 [`SubstError`]。每个错误都携带「现象 / 原因 / 处置」三要素
//! （[`SubstError::elements`]），因为现场唯一无法接受的失败是「换了字体但没人
//! 知道换过」——那既无法归因，也无法回滚。
//!
//! 零静默替换的判定不靠纪律，靠数据结构：
//! * 已安装字体单独登记在 `installed`，命中即「原样使用」，与替换严格区分；
//! * 缺失字体必须有一条显式替换边；没有边就是 [`SubstError::Unregistered`]，
//!   绝不回落到任意默认字体；
//! * 每次解析都产出完整替换链（`Resolution::chain`），链上每一跳都有
//!   [`SubstReason`]，账本 [`SubstLedger`] 把「原样 / 替换 / 拒绝」三分记账，
//!   `silent` 只在替换缺少理由时才会增长——它是审计口径而非常量。
//!
//! 结构清单：
//! * [`FontKey`] 字体身份（族名 + 字重 + 斜体 + 字宽档）；
//! * [`SubstKind`]/[`SubstReason`]/[`License`] 替换性质、动因与目标授权；
//! * [`SubstTable`] 注册表（安装表 + 替换边表，容量、去重、自替换、优先级校验）；
//! * [`Resolution`] 替换链解析结果（终态、跳数、逐跳理由）；
//! * [`SubstLedger`] 账本（三项守恒 + 理由分布 + 零静默判据）；
//! * 注入核 [`injection_core`]：十类坏样本逐类断言具体诊断码。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43700 独占 0x20）。
pub const SEG_BASE: u32 = 0x43700;

pub const E_EMPTY: u16 = 1;
pub const E_DUPLICATE: u16 = 2;
pub const E_FULL: u16 = 3;
pub const E_UNREGISTERED: u16 = 4;
pub const E_IDENTITY: u16 = 5;
pub const E_CYCLE: u16 = 6;
pub const E_DEPTH: u16 = 7;
pub const E_LICENSE: u16 = 8;
pub const E_RANK: u16 = 9;
pub const E_RANGE: u16 = 10;

/// 替换表错误闭集。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubstError {
    Empty,
    Duplicate,
    Full,
    Unregistered,
    Identity,
    Cycle,
    Depth,
    License,
    Rank,
    Range,
}

impl SubstError {
    pub fn code(self) -> u32 {
        let n = match self {
            SubstError::Empty => E_EMPTY,
            SubstError::Duplicate => E_DUPLICATE,
            SubstError::Full => E_FULL,
            SubstError::Unregistered => E_UNREGISTERED,
            SubstError::Identity => E_IDENTITY,
            SubstError::Cycle => E_CYCLE,
            SubstError::Depth => E_DEPTH,
            SubstError::License => E_LICENSE,
            SubstError::Rank => E_RANK,
            SubstError::Range => E_RANGE,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            SubstError::Empty => "font family name is empty",
            SubstError::Duplicate => "source key already registered",
            SubstError::Full => "substitution table capacity exhausted",
            SubstError::Unregistered => "missing font has no explicit substitution",
            SubstError::Identity => "source equals substitution target",
            SubstError::Cycle => "substitution chain loops back",
            SubstError::Depth => "substitution chain exceeds depth limit",
            SubstError::License => "target font license forbids embedding",
            SubstError::Rank => "rank outside 1..=1000",
            SubstError::Range => "weight/stretch outside legal scale",
        }
    }

    /// 错误三要素：(现象, 原因, 处置)。三者都必须非空，否则现场无法闭环。
    pub fn elements(self) -> (&'static str, &'static str, &'static str) {
        match self {
            SubstError::Empty => (
                "替换项族名为空",
                "无名键无法在字体库里定位来源",
                "回填族名后重新登记",
            ),
            SubstError::Duplicate => (
                "同一来源键重复登记",
                "一个来源只能有一条替换边，二义会让解析结果取决于登记顺序",
                "删除旧边再登记，或用 rank 明确优先级",
            ),
            SubstError::Full => (
                "替换表容量已满",
                "无界增长会撑爆定容内核结构",
                "先清理失效条目，再登记新边",
            ),
            SubstError::Unregistered => (
                "缺失字体没有任何替换边",
                "零静默替换要求每一处替换都必须显式登记",
                "登记替代项，或把该字体标记为已安装",
            ),
            SubstError::Identity => (
                "来源与目标为同一字体键",
                "自替换会掩盖真实缺字，解析看似成功却仍缺字",
                "删除自替换边，改登真实替代字体",
            ),
            SubstError::Cycle => (
                "替换链回环",
                "回环会让解析永不收敛，必须当场截断",
                "拆断环中任一条边并改为直达边",
            ),
            SubstError::Depth => (
                "替换链超过深度上限",
                "过长的链多为人手误配，且逐跳都能改写渲染结果",
                "把长链压缩为一条直达边",
            ),
            SubstError::License => (
                "替代字体内嵌许可拒绝",
                "目标授权不允许嵌入输出，换了也不能随文档分发",
                "改用可嵌入替代，或只做屏幕预览",
            ),
            SubstError::Rank => (
                "替换优先级越界",
                "0 表示未赋权，超过上限会溢出排序并改变选边",
                "赋 1..=1000 的显式优先级",
            ),
            SubstError::Range => (
                "字重或字宽档越界",
                "字重仅 100..=900 百位档，字宽档仅 1..=9",
                "归一到合法档位后重试",
            ),
        }
    }
}

/// 字体身份。族名用 String 而非 &str，注册表要跨调用持有它。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FontKey {
    pub family: String,
    pub weight: u16,
    pub italic: bool,
    pub stretch: u8,
}

impl FontKey {
    /// 构造并校验字体键：族名非空、字重 100..=900（百位）、字宽 1..=9。
    pub fn new(family: &str, weight: u16, italic: bool, stretch: u8) -> Result<FontKey, SubstError> {
        let fam = family.trim();
        if fam.is_empty() {
            return Err(SubstError::Empty);
        }
        if weight < 100 || weight > 900 || weight % 100 != 0 {
            return Err(SubstError::Range);
        }
        if stretch < 1 || stretch > 9 {
            return Err(SubstError::Range);
        }
        Ok(FontKey {
            family: fam.to_string(),
            weight,
            italic,
            stretch,
        })
    }

    /// 稳定的短标签，用于账本摘要与调试输出。
    pub fn label(&self) -> String {
        format!(
            "{}-w{}-{}-s{}",
            self.family,
            self.weight,
            if self.italic { "i" } else { "u" },
            self.stretch
        )
    }
}

/// 替换边的性质。用于审计「为什么会换」而不是只看「换成了谁」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubstKind {
    /// 同族不同字重：只做字重钳位。
    WeightClamp,
    /// 同性就近：同风格族内替换。
    FamilyFallback,
    /// 内置兜底档（随产品分发的可嵌入字体）。
    BuiltinFallback,
    /// 最后手段（符号/占位），必须显式标记。
    LastResort,
}

impl SubstKind {
    pub fn code(self) -> u8 {
        match self {
            SubstKind::WeightClamp => 0,
            SubstKind::FamilyFallback => 1,
            SubstKind::BuiltinFallback => 2,
            SubstKind::LastResort => 3,
        }
    }

    pub fn from_code(c: u8) -> Result<SubstKind, SubstError> {
        match c {
            0 => Ok(SubstKind::WeightClamp),
            1 => Ok(SubstKind::FamilyFallback),
            2 => Ok(SubstKind::BuiltinFallback),
            3 => Ok(SubstKind::LastResort),
            _ => Err(SubstError::Range),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SubstKind::WeightClamp => "weight-clamp",
            SubstKind::FamilyFallback => "family-fallback",
            SubstKind::BuiltinFallback => "builtin-fallback",
            SubstKind::LastResort => "last-resort",
        }
    }
}

/// 替换动因。没有动因的替换一律计入 silent，这是 J1 的零静默口径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubstReason {
    NotInstalled,
    LicenseDenied,
    Corrupt,
    CoverageGap,
}

impl SubstReason {
    pub fn code(self) -> u8 {
        match self {
            SubstReason::NotInstalled => 0,
            SubstReason::LicenseDenied => 1,
            SubstReason::Corrupt => 2,
            SubstReason::CoverageGap => 3,
        }
    }

    pub fn from_code(c: u8) -> Result<SubstReason, SubstError> {
        match c {
            0 => Ok(SubstReason::NotInstalled),
            1 => Ok(SubstReason::LicenseDenied),
            2 => Ok(SubstReason::Corrupt),
            3 => Ok(SubstReason::CoverageGap),
            _ => Err(SubstError::Range),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SubstReason::NotInstalled => "not-installed",
            SubstReason::LicenseDenied => "license-denied",
            SubstReason::Corrupt => "corrupt",
            SubstReason::CoverageGap => "coverage-gap",
        }
    }
}

/// 替代字体的内嵌授权档。登记允许，解析「要嵌入」时拒绝——两件事分开。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum License {
    Embeddable,
    PreviewOnly,
    Restricted,
}

impl License {
    pub fn allows_embedding(self) -> bool {
        matches!(self, License::Embeddable)
    }

    pub fn allows_preview(self) -> bool {
        !matches!(self, License::Restricted)
    }

    pub fn label(self) -> &'static str {
        match self {
            License::Embeddable => "embeddable",
            License::PreviewOnly => "preview-only",
            License::Restricted => "restricted",
        }
    }
}

/// 一条替换边：from → to，附带性质、动因、优先级与目标授权。
#[derive(Clone, Debug, PartialEq)]
pub struct SubstEntry {
    pub from: FontKey,
    pub to: FontKey,
    pub kind: SubstKind,
    pub reason: SubstReason,
    pub rank: u16,
    pub license: License,
}

/// 最小跳数上限。8 跳已经覆盖「缺字 → 同族 → 内置 → 最后手段」的合理深度。
pub const MAX_CHAIN: u8 = 8;
/// 优先级上限。rank=0 意为未赋权，直接拒绝。
pub const MAX_RANK: u16 = 1000;

/// 一次成功解析的结果。`chain[0]` 是请求键，末位是终态。
#[derive(Clone, Debug, PartialEq)]
pub struct Resolution {
    pub requested: FontKey,
    pub terminal: FontKey,
    pub hops: u8,
    pub chain: Vec<FontKey>,
    pub reasons: Vec<SubstReason>,
    pub substituted: bool,
}

impl Resolution {
    /// 原样使用（字体已安装），不产生任何替换链。
    pub fn exact(key: FontKey) -> Resolution {
        let mut chain = Vec::with_capacity(1);
        chain.push(key.clone());
        Resolution {
            requested: key.clone(),
            terminal: key,
            hops: 0,
            chain,
            reasons: Vec::new(),
            substituted: false,
        }
    }

    /// 是否直达替代（只换一跳）。
    pub fn is_direct(&self) -> bool {
        self.hops == 1
    }
}

/// 替换注册表：已安装集合 + 显式替换边集合。
#[derive(Clone, Debug)]
pub struct SubstTable {
    entries: Vec<SubstEntry>,
    installed: Vec<FontKey>,
    capacity: usize,
}

impl SubstTable {
    pub fn new(capacity: usize) -> SubstTable {
        SubstTable {
            entries: Vec::new(),
            installed: Vec::new(),
            capacity,
        }
    }

    /// 登记已安装字体。重复登记报 Duplicate，容量按「边 + 安装」合计。
    pub fn install(&mut self, key: FontKey) -> Result<(), SubstError> {
        if self.entries.len() + self.installed.len() >= self.capacity {
            return Err(SubstError::Full);
        }
        let mut i = 0usize;
        while i < self.installed.len() {
            if let Some(k) = self.installed.get(i) {
                if k == &key {
                    return Err(SubstError::Duplicate);
                }
            }
            i += 1;
        }
        self.installed.push(key);
        Ok(())
    }

    /// 登记一条替换边。所有拒绝都在写入前完成，失败不留半条边。
    pub fn register(&mut self, e: SubstEntry) -> Result<(), SubstError> {
        if e.from.family.is_empty() || e.to.family.is_empty() {
            return Err(SubstError::Empty);
        }
        if e.from == e.to {
            return Err(SubstError::Identity);
        }
        if e.rank == 0 || e.rank > MAX_RANK {
            return Err(SubstError::Rank);
        }
        if self.entries.len() + self.installed.len() >= self.capacity {
            return Err(SubstError::Full);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(old) = self.entries.get(i) {
                if old.from == e.from {
                    return Err(SubstError::Duplicate);
                }
            }
            i += 1;
        }
        self.entries.push(e);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn installed_len(&self) -> usize {
        self.installed.len()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 按键查边（来源键唯一，线性查找即可，且不引入额外索引不一致风险）。
    pub fn find(&self, key: &FontKey) -> Option<&SubstEntry> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if &e.from == key {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    pub fn is_installed(&self, key: &FontKey) -> bool {
        let mut i = 0usize;
        while i < self.installed.len() {
            if let Some(k) = self.installed.get(i) {
                if k == key {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 解析替换链。
    ///
    /// 顺序：已安装 → 精确命中；否则沿显式边行走。每跳先查授权（要嵌入时），
    /// 再查深度，最后推进——任何一步失败都返回具体诊断码，绝不回落到默认字体。
    pub fn resolve(&self, key: &FontKey, embedding: bool) -> Result<Resolution, SubstError> {
        if self.is_installed(key) {
            return Ok(Resolution::exact(key.clone()));
        }
        let mut chain: Vec<FontKey> = Vec::with_capacity(4);
        chain.push(key.clone());
        let mut reasons: Vec<SubstReason> = Vec::with_capacity(2);
        let mut seen: Vec<FontKey> = Vec::with_capacity(4);
        let mut cur = key.clone();
        let mut hops: u8 = 0;
        loop {
            let mut revisit = false;
            let mut s = 0usize;
            while s < seen.len() {
                if let Some(k) = seen.get(s) {
                    if k == &cur {
                        revisit = true;
                    }
                }
                s += 1;
            }
            if revisit {
                return Err(SubstError::Cycle);
            }
            seen.push(cur.clone());
            let next = match self.find(&cur) {
                Some(e) => e,
                None => break,
            };
            if embedding && !next.license.allows_embedding() {
                return Err(SubstError::License);
            }
            if hops >= MAX_CHAIN {
                return Err(SubstError::Depth);
            }
            reasons.push(next.reason);
            cur = next.to.clone();
            chain.push(cur.clone());
            hops += 1;
        }
        if hops == 0 {
            return Err(SubstError::Unregistered);
        }
        Ok(Resolution {
            requested: key.clone(),
            terminal: cur,
            hops,
            chain,
            reasons,
            substituted: true,
        })
    }
}

/// 账本行结果：原样 / 替换 / 拒绝（携带诊断码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Exact,
    Substituted,
    Rejected(u16),
}

/// 一行解析记录。`reason` 为 None 而结果是替换时，计入 silent。
#[derive(Clone, Debug, PartialEq)]
pub struct LedgerRow {
    pub requested: FontKey,
    pub terminal: FontKey,
    pub outcome: Outcome,
    pub hops: u8,
    pub reason: Option<SubstReason>,
}

/// 替换账本：三项守恒 + 理由分布 + 零静默审计。
#[derive(Clone, Debug)]
pub struct SubstLedger {
    rows: Vec<LedgerRow>,
    pub attempts: u32,
    pub exact: u32,
    pub substituted: u32,
    pub rejected: u32,
    pub silent: u32,
    pub reason_counts: [u32; 4],
}

impl SubstLedger {
    pub fn new() -> SubstLedger {
        SubstLedger {
            rows: Vec::new(),
            attempts: 0,
            exact: 0,
            substituted: 0,
            rejected: 0,
            silent: 0,
            reason_counts: [0; 4],
        }
    }

    /// 记录一次解析。Conservation 的前提是每条路径恰好落一格。
    pub fn record(&mut self, key: &FontKey, result: &Result<Resolution, SubstError>) {
        self.attempts += 1;
        match result {
            Ok(r) => {
                if r.substituted {
                    self.substituted += 1;
                    match r.reasons.first() {
                        Some(reason) => {
                            let idx = reason.code() as usize;
                            if let Some(cell) = self.reason_counts.get_mut(idx) {
                                *cell += 1;
                            }
                        }
                        None => self.silent += 1,
                    }
                    self.rows.push(LedgerRow {
                        requested: key.clone(),
                        terminal: r.terminal.clone(),
                        outcome: Outcome::Substituted,
                        hops: r.hops,
                        reason: r.reasons.first().copied(),
                    });
                } else {
                    self.exact += 1;
                    self.rows.push(LedgerRow {
                        requested: key.clone(),
                        terminal: r.terminal.clone(),
                        outcome: Outcome::Exact,
                        hops: 0,
                        reason: None,
                    });
                }
            }
            Err(e) => {
                self.rejected += 1;
                self.rows.push(LedgerRow {
                    requested: key.clone(),
                    terminal: key.clone(),
                    outcome: Outcome::Rejected(e.code() as u16),
                    hops: 0,
                    reason: None,
                });
            }
        }
    }

    /// 守恒：每次尝试恰好计入一项，不多不少。
    pub fn conservation_ok(&self) -> bool {
        self.attempts == self.exact + self.substituted + self.rejected
            && self.attempts as usize == self.rows.len()
    }

    /// 零静默：所有替换都带动因，且理由分布之和等于替换次数。
    pub fn zero_silent(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.reason_counts.len() {
            if let Some(c) = self.reason_counts.get(i) {
                sum += *c;
            }
            i += 1;
        }
        self.silent == 0 && sum == self.substituted
    }

    pub fn rows(&self) -> &[LedgerRow] {
        &self.rows
    }

    /// 摘要必须包含可审计字段；空摘要等于没记账。
    pub fn summary(&self) -> String {
        format!(
            "attempts={} exact={} subst={} rejected={} silent={}",
            self.attempts, self.exact, self.substituted, self.rejected, self.silent
        )
    }
}

// ---------------------------------------------------------------------------
// 注入核：十类坏样本，逐类断言具体诊断码
// ---------------------------------------------------------------------------

fn must_key(family: &str, weight: u16) -> FontKey {
    match FontKey::new(family, weight, false, 5) {
        Ok(k) => k,
        Err(_) => FontKey {
            family: "fallback".to_string(),
            weight: 400,
            italic: false,
            stretch: 5,
        },
    }
}

fn edge(from: FontKey, to: FontKey, rank: u16, license: License) -> SubstEntry {
    SubstEntry {
        from,
        to,
        kind: SubstKind::FamilyFallback,
        reason: SubstReason::NotInstalled,
        rank,
        license,
    }
}

/// 注入核。返回（被抓数, 总数）；每条都对照一个具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 10usize;

    // 1. 空族名
    if FontKey::new("   ", 400, false, 5).err() == Some(SubstError::Empty) {
        caught += 1;
    }
    // 2. 字重越界
    if FontKey::new("Arial", 250, false, 5).err() == Some(SubstError::Range) {
        caught += 1;
    }
    // 3. 自替换
    let mut t = SubstTable::new(32);
    let a = must_key("Alpha", 400);
    if t.register(edge(a.clone(), a.clone(), 10, License::Embeddable))
        .err()
        == Some(SubstError::Identity)
    {
        caught += 1;
    }
    // 4. 重复来源
    let b = must_key("Beta", 400);
    let c = must_key("Gamma", 400);
    let ok1 = t.register(edge(a.clone(), b.clone(), 10, License::Embeddable)).is_ok();
    let dup = t.register(edge(a.clone(), c.clone(), 20, License::Embeddable)).err();
    if ok1 && dup == Some(SubstError::Duplicate) {
        caught += 1;
    }
    // 5. 回环 A→B→A
    let mut cyc = SubstTable::new(16);
    let x = must_key("CycleA", 400);
    let y = must_key("CycleB", 400);
    let r1 = cyc.register(edge(x.clone(), y.clone(), 10, License::Embeddable)).is_ok();
    let r2 = cyc.register(edge(y.clone(), x.clone(), 20, License::Embeddable)).is_ok();
    if r1 && r2 && cyc.resolve(&x, true).err() == Some(SubstError::Cycle) {
        caught += 1;
    }
    // 6. 未登记缺失字体
    let empty = SubstTable::new(4);
    let ghost = must_key("Ghost", 400);
    if empty.resolve(&ghost, true).err() == Some(SubstError::Unregistered) {
        caught += 1;
    }
    // 7. 许可拒绝（受限目标，要求嵌入）
    let mut lic = SubstTable::new(8);
    let src = must_key("Paid", 400);
    let dst = must_key("Restricted", 400);
    let reg = lic.register(edge(src.clone(), dst.clone(), 10, License::Restricted)).is_ok();
    if reg && lic.resolve(&src, true).err() == Some(SubstError::License) {
        caught += 1;
    }
    // 8. 深度上限：9 条链边
    let mut deep = SubstTable::new(64);
    let mut i = 0u16;
    let mut chain_ok = true;
    while i <= 9 {
        let f = must_key(&format!("DeepA{}", i), 400);
        let to = must_key(&format!("DeepB{}", i), 400);
        if deep.register(edge(f, to, 10, License::Embeddable)).is_err() {
            chain_ok = false;
        }
        i += 1;
    }
    // 前一条边的目标是下一条边的来源，构造 10 跳链。
    let mut j = 0u16;
    while j < 9 {
        let from = must_key(&format!("DeepB{}", j), 400);
        let to = must_key(&format!("DeepA{}", j + 1), 400);
        if deep.register(edge(from, to, 10, License::Embeddable)).is_err() {
            chain_ok = false;
        }
        j += 1;
    }
    if chain_ok && deep.resolve(&must_key("DeepA0", 400), true).err() == Some(SubstError::Depth) {
        caught += 1;
    }
    // 9. 容量满
    let mut small = SubstTable::new(1);
    let s1 = must_key("SmallA", 400);
    let s2 = must_key("SmallB", 400);
    let s3 = must_key("SmallC", 400);
    let first = small.register(edge(s1, s2, 10, License::Embeddable)).is_ok();
    if first && small.register(edge(s3, must_key("SmallD", 400), 10, License::Embeddable)).err() == Some(SubstError::Full) {
        caught += 1;
    }
    // 10. 优先级 0
    let mut rk = SubstTable::new(4);
    let ra = must_key("RankA", 400);
    let rb = must_key("RankB", 400);
    if rk.register(edge(ra, rb, 0, License::Embeddable)).err() == Some(SubstError::Rank) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

fn build_zoo() -> (SubstTable, Vec<FontKey>, Vec<FontKey>) {
    let mut table = SubstTable::new(64);
    let kinds = [
        SubstKind::WeightClamp,
        SubstKind::FamilyFallback,
        SubstKind::BuiltinFallback,
        SubstKind::LastResort,
    ];
    let reasons = [
        SubstReason::NotInstalled,
        SubstReason::LicenseDenied,
        SubstReason::Corrupt,
        SubstReason::CoverageGap,
    ];
    let mut sources: Vec<FontKey> = Vec::with_capacity(20);
    let mut targets: Vec<FontKey> = Vec::with_capacity(20);
    let mut i = 0usize;
    while i < 20 {
        let src = must_key(&format!("Ghost{:02}", i), 400);
        let dst = must_key(&format!("Fallback{:02}", i), if i % 2 == 0 { 400 } else { 700 });
        let e = SubstEntry {
            from: src.clone(),
            to: dst.clone(),
            kind: kinds[i % kinds.len()],
            reason: reasons[i % reasons.len()],
            rank: (i as u16 % 50) + 1,
            license: License::Embeddable,
        };
        let _ = table.register(e);
        sources.push(src);
        targets.push(dst);
        i += 1;
    }
    (table, sources, targets)
}

pub fn run_unx50087_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50087");
    let (table, sources, _targets) = build_zoo();

    // J1-1 替换 ×20 显式记账：每条都解析成功且带跳数与理由
    let mut ledger = SubstLedger::new();
    let mut all_subst = true;
    let mut all_reason = true;
    let mut i = 0usize;
    while i < sources.len() {
        if let Some(src) = sources.get(i) {
            let r = table.resolve(src, true);
            match &r {
                Ok(res) => {
                    if !res.substituted || res.hops < 1 {
                        all_subst = false;
                    }
                    if res.reasons.is_empty() {
                        all_reason = false;
                    }
                }
                Err(_) => all_subst = false,
            }
            ledger.record(src, &r);
        }
        i += 1;
    }
    let ok = all_subst
        && all_reason
        && ledger.substituted == 20
        && ledger.exact == 0
        && ledger.rejected == 0
        && ledger.silent == 0
        && ledger.rows().len() == 20;
    let det = format!(
        "subst={} silent={} rows={}",
        ledger.substituted,
        ledger.silent,
        ledger.rows().len()
    );
    s.add_dyn("50087-J1-替换×20记账", ok, &det);

    // J1-2 零静默：理由分布四项都出现且总数等于替换次数
    let mut nonzero = 0usize;
    let mut k = 0usize;
    while k < ledger.reason_counts.len() {
        if let Some(c) = ledger.reason_counts.get(k) {
            if *c > 0 {
                nonzero += 1;
            }
        }
        k += 1;
    }
    let ok = ledger.zero_silent() && nonzero == 4;
    let det = format!("reasons={:?}", ledger.reason_counts);
    s.add_dyn("50087-J1-零静默", ok, &det);

    // J1-3 已安装字体原样使用，不计入替换
    let mut inst = SubstTable::new(8);
    let present = must_key("Present", 400);
    let _ = inst.install(present.clone());
    let mut il = SubstLedger::new();
    let r = inst.resolve(&present, true);
    il.record(&present, &r);
    let ok = match r {
        Ok(res) => !res.substituted && res.hops == 0 && il.exact == 1 && il.substituted == 0,
        Err(_) => false,
    };
    s.add("50087-J1-已安装原样", ok, if ok { "" } else { "已安装被误记为替换" });

    // J1-4 未登记缺失字体必须显式失败（零静默替换的核心）
    let missing = must_key("Missing", 400);
    let mut ml = SubstLedger::new();
    let mr = inst.resolve(&missing, true);
    ml.record(&missing, &mr);
    let ok = mr.err() == Some(SubstError::Unregistered)
        && ml.rejected == 1
        && ml.substituted == 0
        && ml.conservation_ok();
    s.add("50087-J1-未登记必拒", ok, if ok { "" } else { "缺失字体被静默放过" });

    // J1-5 重复来源拒绝，且不产生第二条边
    let mut dup = SubstTable::new(8);
    let d1 = must_key("DupA", 400);
    let d2 = must_key("DupB", 400);
    let d3 = must_key("DupC", 400);
    let first = dup.register(edge(d1.clone(), d2, 10, License::Embeddable)).is_ok();
    let second = dup.register(edge(d1, d3, 20, License::Embeddable)).err();
    let ok = first && second == Some(SubstError::Duplicate) && dup.len() == 1;
    s.add("50087-J1-重复来源拒绝", ok, if ok { "" } else { "重复边被收" });

    // J1-6 容量满拒绝
    let mut full = SubstTable::new(1);
    let f1 = must_key("FullA", 400);
    let f2 = must_key("FullB", 400);
    let f3 = must_key("FullC", 400);
    let f4 = must_key("FullD", 400);
    let a = full.register(edge(f1, f2, 10, License::Embeddable)).is_ok();
    let b = full.register(edge(f3, f4, 10, License::Embeddable)).err();
    let ok = a && b == Some(SubstError::Full) && full.len() == 1;
    s.add("50087-J1-容量满拒绝", ok, if ok { "" } else { "超容被收" });

    // J1-7 自替换拒绝
    let mut ident = SubstTable::new(4);
    let idk = must_key("Self", 400);
    let ok = ident
        .register(edge(idk.clone(), idk, 5, License::Embeddable))
        .err()
        == Some(SubstError::Identity);
    s.add("50087-J1-自替换拒绝", ok, if ok { "" } else { "自替换被收" });

    // J1-8 回环检测
    let mut cyc = SubstTable::new(8);
    let ca = must_key("CycleA", 400);
    let cb = must_key("CycleB", 400);
    let r1 = cyc.register(edge(ca.clone(), cb.clone(), 10, License::Embeddable)).is_ok();
    let r2 = cyc.register(edge(cb, ca.clone(), 20, License::Embeddable)).is_ok();
    let ok = r1 && r2 && cyc.resolve(&ca, true).err() == Some(SubstError::Cycle);
    s.add("50087-J1-回环检测", ok, if ok { "" } else { "回环未被截断" });

    // J1-9 深度上限：9 跳链在 8 跳处失败
    let mut deep = SubstTable::new(32);
    let mut n = 0u16;
    while n < 9 {
        let a = must_key(&format!("Chain{}", n), 400);
        let b = must_key(&format!("Chain{}", n + 1), 400);
        let _ = deep.register(edge(a, b, 10, License::Embeddable));
        n += 1;
    }
    let ok = deep.resolve(&must_key("Chain0", 400), true).err() == Some(SubstError::Depth);
    s.add("50087-J1-深度上限", ok, if ok { "" } else { "超深链被放行" });

    // J1-10 许可：嵌入拒绝、预览放行
    let mut lic = SubstTable::new(8);
    let ls = must_key("PaidFace", 400);
    let lt = must_key("PreviewFace", 400);
    let reg = lic.register(edge(ls.clone(), lt.clone(), 10, License::PreviewOnly)).is_ok();
    let embed = lic.resolve(&ls, true).err();
    let preview = lic.resolve(&ls, false);
    let ok = reg
        && embed == Some(SubstError::License)
        && match preview {
            Ok(res) => res.substituted && res.terminal == lt,
            Err(_) => false,
        };
    s.add("50087-J1-许可分档", ok, if ok { "" } else { "许可档位未生效" });

    // J1-11 优先级/字重档位校验
    let mut rank = SubstTable::new(8);
    let ra = must_key("RankA", 400);
    let rb = must_key("RankB", 400);
    let rank_err = rank.register(edge(ra.clone(), rb.clone(), 0, License::Embeddable)).err();
    let range_err = FontKey::new("RangeA", 1000, false, 5).err();
    let stretch_err = FontKey::new("RangeB", 400, false, 0).err();
    let ok = rank_err == Some(SubstError::Rank)
        && range_err == Some(SubstError::Range)
        && stretch_err == Some(SubstError::Range)
        && rank.len() == 0;
    s.add("50087-J1-档位校验", ok, if ok { "" } else { "非法档位被收" });

    // J1-12 多跳链：A→B→C，终态 C、跳数 2、理由 2 条
    let mut multi = SubstTable::new(8);
    let ma = must_key("MultiA", 400);
    let mb = must_key("MultiB", 400);
    let mc = must_key("MultiC", 400);
    let _ = multi.register(edge(ma.clone(), mb.clone(), 10, License::Embeddable));
    let _ = multi.register(edge(mb, mc.clone(), 20, License::Embeddable));
    let ok = match multi.resolve(&ma, true) {
        Ok(res) => {
            res.terminal == mc
                && res.hops == 2
                && res.chain.len() == 3
                && res.reasons.len() == 2
                && res.is_direct() == false
        }
        Err(_) => false,
    };
    s.add("50087-J1-多跳链", ok, if ok { "" } else { "多跳链结果错" });

    // J1-13 守恒：混合原样/替换/拒绝后三项之和等于尝试数
    let mut mix = SubstTable::new(16);
    let present2 = must_key("MixPresent", 400);
    let _ = mix.install(present2.clone());
    let ms = must_key("MixMissing", 400);
    let mt = must_key("MixTarget", 400);
    let _ = mix.register(edge(ms.clone(), mt, 10, License::Embeddable));
    let unknown = must_key("MixUnknown", 400);
    let mut ml2 = SubstLedger::new();
    let rr = mix.resolve(&present2, true);
    ml2.record(&present2, &rr);
    let rr2 = mix.resolve(&ms, true);
    ml2.record(&ms, &rr2);
    let rr3 = mix.resolve(&unknown, true);
    ml2.record(&unknown, &rr3);
    let ok = ml2.exact == 1
        && ml2.substituted == 1
        && ml2.rejected == 1
        && ml2.attempts == 3
        && ml2.conservation_ok();
    let det = format!("{}/{}/{}/{}", ml2.exact, ml2.substituted, ml2.rejected, ml2.attempts);
    s.add_dyn("50087-J1-三项守恒", ok, &det);

    // J1-14 错误三要素：十条错误全部有现象/原因/处置，码值落在段内
    let all = [
        SubstError::Empty,
        SubstError::Duplicate,
        SubstError::Full,
        SubstError::Unregistered,
        SubstError::Identity,
        SubstError::Cycle,
        SubstError::Depth,
        SubstError::License,
        SubstError::Rank,
        SubstError::Range,
    ];
    let mut i2 = 0usize;
    let mut ok = true;
    while i2 < all.len() {
        if let Some(e) = all.get(i2) {
            let (sym, cause, fix) = e.elements();
            if sym.is_empty() || cause.is_empty() || fix.is_empty() || e.message().is_empty() {
                ok = false;
            }
            let expect = SEG_BASE + (i2 as u32) + 1;
            if e.code() != expect {
                ok = false;
            }
        }
        i2 += 1;
    }
    s.add("50087-J1-错误三要素", ok, if ok { "" } else { "三要素缺失或码值漂移" });

    // J1-15 解析确定性：同输入两次逐字段一致；账本摘要稳定
    let dk = must_key("Ghost07", 400);
    let dr1 = table.resolve(&dk, true);
    let dr2 = table.resolve(&dk, true);
    let det_ok = match (&dr1, &dr2) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    let sum1 = ledger.summary();
    let sum2 = ledger.summary();
    let ok = det_ok && sum1 == sum2 && sum1.contains("subst=20") && sum1.contains("silent=0");
    s.add("50087-J1-解析确定", ok, if ok { "" } else { "两次解析不一致" });

    // J1-16 注入核十类全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50087-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50087_checks_green() {
        let set = run_unx50087_checks();
        assert!(set.all_passed(), "UNX-F50087 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50087_chain_is_explicit() {
        let mut t = SubstTable::new(8);
        let a = must_key("A", 400);
        let b = must_key("B", 700);
        let c = must_key("C", 400);
        assert!(t.register(edge(a.clone(), b.clone(), 1, License::Embeddable)).is_ok());
        assert!(t.register(edge(b, c.clone(), 2, License::Embeddable)).is_ok());
        match t.resolve(&a, true) {
            Ok(r) => {
                assert_eq!(r.terminal, c);
                assert_eq!(r.hops, 2);
                assert_eq!(r.reasons.len(), 2);
            }
            Err(_) => panic!("explicit chain must resolve"),
        }
    }

    #[test]
    fn unx50087_zero_silent_holds() {
        let (table, sources, _) = build_zoo();
        let mut ledger = SubstLedger::new();
        for k in sources.iter() {
            let r = table.resolve(k, true);
            ledger.record(k, &r);
        }
        assert_eq!(ledger.substituted, 20);
        assert!(ledger.zero_silent());
        assert!(ledger.conservation_ok());
    }
}
