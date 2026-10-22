//! VE-F3440 · E02 组收口 —— 名录完整性、两律、双签、证据号、包哈希
//!
//! ## 职责拆解
//! 1. **名录**（`Roster`）：E02 共 20 条（VE-F3421~VE-F3440）。收口的第一件事不是
//!    看绿不绿，而是**证明这 20 条都在**。抽查五条全绿就宣布收口，是收口最经典的
//!    作假方式——漏掉的正好是没做的那几条。
//! 2. **六件**（`Artifact`）：总纲、门面、fuzz、文档、基准、判据。每条必须六件齐备；
//!    `Artifacts` 用位图而非六个 bool，缺哪件直接报枚举，不用猜。
//! 3. **两律**（`Law`）：E02 的两条不可协商的性质——换肤不丢状态、失败必回滚。
//!    每条子项必须至少推进其中一律并给出证据；两律合起来覆盖全组，缺一律即红。
//! 4. **对拍归档**（`diff_archived`）：差异表必须落盘。只在 CI 里跑一遍不算——
//!    「上次是多少」没有记录，回归就无从判定。
//! 5. **fuzz 重跑**（`RerunProof`）：双签前必须重跑全量语料，且必须是新鲜的
//!    （`FUZZ_FRESH_TICKS` 内）。半年前的绿色报告不能替今天的代码背书。
//! 6. **双签**（`Signature`）：两人、两个角色、同一个包哈希。同名同角色同哈希都拒。
//! 7. **证据号**（`EvidenceBook`）：`EV-E02-0001` 起单调递增，**红门不发号**。
//!    号一旦发出就不能回收——所以发号必须发生在全部检查之后，这是顺序性要求。
//!
//! ## 为什么这么拆
//! 收口的本质是「把 20 条子项的 20 份信心合成 1 份可追责的结论」。合成过程中最容易
//! 丢的三样东西是：名录（漏条）、证据（口头绿）、时间（旧报告）。所以本册把三者
//! 都做成结构：名录要连续、证据要编号、fuzz 要新鲜。
//!
//! ## 复杂度
//! - 名录校验：排序 O(n log n)，n=20。
//! - 六件/两律：O(n·6)。
//! - 包哈希：O(n·len)，逐行 FNV。
//! 全量 < 1 万次操作，微秒级。
//!
//! ## 零 panic 面
//! - 无索引切片；枚举穷举 `match`；无 `unwrap` / `expect`。
//! - 哈希全 `wrapping_mul`；计数用 `saturating_*`。
//! - `EvidenceBook::issue` 溢出时夹在 `u32::MAX` 并不再自增（宁可发重号也不 panic）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 组内首条编号。
pub const GROUP_FIRST: u32 = 3421;
/// 组内末条编号。
pub const GROUP_LAST: u32 = 3440;
/// 组内条数。与首末编号自洽（3440-3421+1 = 20），测试会交叉验证。
pub const GROUP_SIZE: usize = 20;
/// 证据号前缀。
pub const EVIDENCE_PREFIX: &str = "EV-E02-";
/// fuzz 重跑的新鲜窗：7200 个 tick，按 1 秒 tick 计为两小时。
pub const FUZZ_FRESH_TICKS: u64 = 7200;

/// 诊断码（E18 段 · 5569~5580）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum CloseCode {
    /// 名录条数与组规模不符。
    RosterSize = 5569,
    /// 名录编号不连续（漏条或多条）。
    RosterGap = 5570,
    /// 某条判据未全绿。
    ChecksRed = 5571,
    /// 某条未推进缺席的那一律。
    LawUnproven = 5572,
    /// 某条缺对拍归档。
    DiffMissing = 5573,
    /// 某条六件不齐。
    ArtifactMissing = 5574,
    /// fuzz 未重跑。
    FuzzMissing = 5575,
    /// fuzz 重跑不新鲜。
    FuzzStale = 5576,
    /// 签署人不足两人。
    SignerCount = 5577,
    /// 两个签署人角色相同。
    SignerRole = 5578,
    /// 签署人签的不是这个包。
    SignerHash = 5579,
    /// 包哈希与名录不一致（被改过）。
    PackageTampered = 5580,
}

impl CloseCode {
    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn text(self) -> &'static str {
        match self {
            CloseCode::RosterSize => "名录条数与组规模不符",
            CloseCode::RosterGap => "名录编号不连续",
            CloseCode::ChecksRed => "判据未全绿",
            CloseCode::LawUnproven => "两律未全部落实",
            CloseCode::DiffMissing => "缺对拍归档",
            CloseCode::ArtifactMissing => "六件不齐",
            CloseCode::FuzzMissing => "双签前未重跑 fuzz",
            CloseCode::FuzzStale => "fuzz 报告过期",
            CloseCode::SignerCount => "签署人不足两人",
            CloseCode::SignerRole => "签署人角色相同",
            CloseCode::SignerHash => "签署人与包哈希不符",
            CloseCode::PackageTampered => "包哈希与名录不一致",
        }
    }
}

/// E02 的两律。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Law {
    /// 换肤不丢状态：切换前后交互状态、滚动位置、焦点守恒。
    StateKept,
    /// 失败必回滚：任何失败路径都不得留下半新半旧。
    MustRollback,
}

impl Law {
    pub fn index(self) -> usize {
        match self {
            Law::StateKept => 0,
            Law::MustRollback => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Law::StateKept => "换肤不丢状态",
            Law::MustRollback => "失败必回滚",
        }
    }

    pub const ALL: [Law; 2] = [Law::StateKept, Law::MustRollback];
}

/// 六件。名字按 E02 的真实交付物取，不用抽象编号——抽象编号让人记不住缺的是哪件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Artifact {
    /// 总纲（F3421）。
    Spec,
    /// 门面/运行时（F3422~F3435）。
    Facade,
    /// fuzz 语料（F3438）。
    Fuzz,
    /// 文档（F3436）。
    Docs,
    /// 基准（F3439）。
    Bench,
    /// 判据（本条）。
    Checks,
}

impl Artifact {
    pub fn bit(self) -> u8 {
        match self {
            Artifact::Spec => 1 << 0,
            Artifact::Facade => 1 << 1,
            Artifact::Fuzz => 1 << 2,
            Artifact::Docs => 1 << 3,
            Artifact::Bench => 1 << 4,
            Artifact::Checks => 1 << 5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Artifact::Spec => "总纲",
            Artifact::Facade => "门面",
            Artifact::Fuzz => "fuzz",
            Artifact::Docs => "文档",
            Artifact::Bench => "基准",
            Artifact::Checks => "判据",
        }
    }

    pub const ALL: [Artifact; 6] = [
        Artifact::Spec,
        Artifact::Facade,
        Artifact::Fuzz,
        Artifact::Docs,
        Artifact::Bench,
        Artifact::Checks,
    ];
}

/// 六件位图。用位图而不是 `[bool; 6]`：缺件可以一眼报出枚举名。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Artifacts(u8);

impl Artifacts {
    pub const fn none() -> Self {
        Self(0)
    }

    pub const fn full() -> Self {
        Self(0b0011_1111)
    }

    pub fn with(mut self, a: Artifact) -> Self {
        self.0 |= a.bit();
        self
    }

    pub fn has(self, a: Artifact) -> bool {
        self.0 & a.bit() != 0
    }

    pub fn count(self) -> u32 {
        self.0.count_ones()
    }

    /// 第一个缺件。调用方逐个修复时不必自己遍历。
    pub fn first_missing(self) -> Option<Artifact> {
        Artifact::ALL.iter().copied().find(|a| !self.has(*a))
    }

    pub fn is_full(self) -> bool {
        self.first_missing().is_none()
    }
}

/// 一条子项的收口台账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItemRow {
    pub id: u32,
    pub green: bool,
    pub laws: [bool; 2],
    pub diff_archived: bool,
    pub artifacts: Artifacts,
}

impl ItemRow {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            green: false,
            laws: [false, false],
            diff_archived: false,
            artifacts: Artifacts::none(),
        }
    }

    /// 出厂的「全绿」形态，供 `standard_package` 与测试夹具使用。
    pub fn green(id: u32) -> Self {
        Self {
            id,
            green: true,
            laws: [true, true],
            diff_archived: true,
            artifacts: Artifacts::full(),
        }
    }

    /// 是否至少推进了一律——这是「两律合起来覆盖全组」的分母。
    pub fn advances_any_law(&self) -> bool {
        self.laws.iter().any(|v| *v)
    }

    fn canonical(&self) -> String {
        format!(
            "{}:{}:{}{}:{}:{:02x}",
            self.id,
            if self.green { 1 } else { 0 },
            if self.laws[0] { 1 } else { 0 },
            if self.laws[1] { 1 } else { 0 },
            if self.diff_archived { 1 } else { 0 },
            self.artifacts.0
        )
    }
}

/// 缺件原因。带编号，报告里能直接指到条。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Miss {
    RosterSize { expected: usize, found: usize },
    RosterGap { missing: u32 },
    ChecksRed { id: u32 },
    LawUnproven { id: u32, law: Law },
    DiffMissing { id: u32 },
    ArtifactMissing { id: u32, which: Artifact },
    FuzzMissing,
    FuzzStale { age_ticks: u64 },
    SignerCount { found: usize },
    SignerRole,
    SignerHash { name: &'static str },
    PackageTampered { stored: u64, computed: u64 },
}

impl Miss {
    pub fn code(&self) -> CloseCode {
        match self {
            Miss::RosterSize { .. } => CloseCode::RosterSize,
            Miss::RosterGap { .. } => CloseCode::RosterGap,
            Miss::ChecksRed { .. } => CloseCode::ChecksRed,
            Miss::LawUnproven { .. } => CloseCode::LawUnproven,
            Miss::DiffMissing { .. } => CloseCode::DiffMissing,
            Miss::ArtifactMissing { .. } => CloseCode::ArtifactMissing,
            Miss::FuzzMissing => CloseCode::FuzzMissing,
            Miss::FuzzStale { .. } => CloseCode::FuzzStale,
            Miss::SignerCount { .. } => CloseCode::SignerCount,
            Miss::SignerRole => CloseCode::SignerRole,
            Miss::SignerHash { .. } => CloseCode::SignerHash,
            Miss::PackageTampered { .. } => CloseCode::PackageTampered,
        }
    }

    /// 是否为「提示级」：fuzz 报告过期属提示，可以带条件收口；
    /// 但**红门不发号**这条不变——提示级只影响门色，不影响证据号的严肃性。
    pub fn is_advisory(&self) -> bool {
        matches!(self, Miss::FuzzStale { .. })
    }

    pub fn text(&self) -> String {
        match self {
            Miss::RosterSize { expected, found } => {
                format!("名录 {} 条，应为 {} 条", found, expected)
            }
            Miss::RosterGap { missing } => format!("缺 VE-F{}", missing),
            Miss::ChecksRed { id } => format!("VE-F{} 判据未全绿", id),
            Miss::LawUnproven { id, law } => {
                format!("VE-F{} 未落实「{}」", id, law.label())
            }
            Miss::DiffMissing { id } => format!("VE-F{} 缺对拍归档", id),
            Miss::ArtifactMissing { id, which } => {
                format!("VE-F{} 缺{}", id, which.label())
            }
            Miss::FuzzMissing => "双签前未重跑 fuzz".to_string(),
            Miss::FuzzStale { age_ticks } => {
                format!("fuzz 报告已 {} tick，超出新鲜窗 {}", age_ticks, FUZZ_FRESH_TICKS)
            }
            Miss::SignerCount { found } => format!("签署人 {} 人，需 2 人", found),
            Miss::SignerRole => "两名签署人角色相同".to_string(),
            Miss::SignerHash { name } => format!("{} 签的不是这个包", name),
            Miss::PackageTampered { stored, computed } => format!(
                "包哈希不符：记录 {:016x}，重算 {:016x}",
                stored, computed
            ),
        }
    }
}

/// fuzz 重跑证明。`families` 必须覆盖三族（畸形/风暴/竞态），少一族等于少一条腿。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RerunProof {
    pub families: u8,
    pub cases: usize,
    pub verdict_green: bool,
    pub finished_at: u64,
}

impl RerunProof {
    pub const FAMILIES_REQUIRED: u8 = 3;

    pub fn is_fresh(&self, now: u64) -> bool {
        now.saturating_sub(self.finished_at) <= FUZZ_FRESH_TICKS
    }

    pub fn is_complete(&self) -> bool {
        self.families >= Self::FAMILIES_REQUIRED && self.cases > 0 && self.verdict_green
    }
}

/// 签署角色。两个角色必须不同——同角色双签等于一个人签两次。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Owner,
    Reviewer,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Owner => "责任人",
            Role::Reviewer => "复核人",
        }
    }
}

/// 一条签署。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Signature {
    pub name: &'static str,
    pub role: Role,
    pub package_hash: u64,
}

/// 收口包。
#[derive(Clone, Debug, Default)]
pub struct Package {
    pub rows: Vec<ItemRow>,
    pub fuzz: Option<RerunProof>,
    pub signatures: Vec<Signature>,
    /// 封包时算出的哈希。`None` 表示未封包。
    pub sealed_hash: Option<u64>,
}

impl Package {
    /// 计算包哈希：名录逐行规范化后 FNV-1a 64。
    /// 只要有一条子项的状态变了，哈希就变——签署签的是这个数。
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for row in &self.rows {
            for byte in row.canonical().as_bytes() {
                h ^= u64::from(*byte);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            h ^= 0xFF;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// 封包：把哈希固定下来。封包之后再改名录，审计会报 `PackageTampered`。
    pub fn seal(&mut self) -> u64 {
        let h = self.hash();
        self.sealed_hash = Some(h);
        h
    }

    /// 审计：返回全部缺件。**不做短路**——一次把所有问题报全，
    /// 否则修复会变成「报一个修一个」的 N 轮往返。
    pub fn audit(&self, now: u64) -> Vec<Miss> {
        let mut misses = Vec::new();

        // 1. 名录完整性：条数 + 连续编号。
        if self.rows.len() != GROUP_SIZE {
            misses.push(Miss::RosterSize {
                expected: GROUP_SIZE,
                found: self.rows.len(),
            });
        }
        for id in GROUP_FIRST..=GROUP_LAST {
            if !self.rows.iter().any(|r| r.id == id) {
                misses.push(Miss::RosterGap { missing: id });
            }
        }

        // 2. 逐条：全绿 / 两律 / 对拍 / 六件。
        for row in &self.rows {
            if !row.green {
                misses.push(Miss::ChecksRed { id: row.id });
            }
            for law in Law::ALL {
                if !row.laws[law.index()] {
                    misses.push(Miss::LawUnproven { id: row.id, law });
                }
            }
            if !row.diff_archived {
                misses.push(Miss::DiffMissing { id: row.id });
            }
            if let Some(which) = row.artifacts.first_missing() {
                misses.push(Miss::ArtifactMissing {
                    id: row.id,
                    which,
                });
            }
        }

        // 3. 两律全组覆盖：允许个别条只推进一律，但不允许全组没人推进某一律。
        for law in Law::ALL {
            if !self.rows.iter().any(|r| r.laws[law.index()]) {
                // 用首条编号占位，报告里说清是「全组缺席」。
                misses.push(Miss::LawUnproven {
                    id: GROUP_FIRST,
                    law,
                });
            }
        }

        // 4. fuzz 重跑。
        match &self.fuzz {
            None => misses.push(Miss::FuzzMissing),
            Some(p) => {
                if !p.is_complete() {
                    misses.push(Miss::FuzzMissing);
                } else if !p.is_fresh(now) {
                    misses.push(Miss::FuzzStale {
                        age_ticks: now.saturating_sub(p.finished_at),
                    });
                }
            }
        }

        // 5. 双签：两人、两角色、同包哈希。
        if self.signatures.len() < 2 {
            misses.push(Miss::SignerCount {
                found: self.signatures.len(),
            });
        } else {
            let mut names: Vec<&'static str> = Vec::new();
            let mut roles: Vec<Role> = Vec::new();
            for s in &self.signatures {
                if names.contains(&s.name) || roles.contains(&s.role) {
                    misses.push(Miss::SignerRole);
                }
                names.push(s.name);
                roles.push(s.role);
            }
            if let Some(h) = self.sealed_hash {
                for s in &self.signatures {
                    if s.package_hash != h {
                        misses.push(Miss::SignerHash { name: s.name });
                    }
                }
            }
        }

        // 6. 哈希自洽（封包后是否被改过）。
        if let Some(stored) = self.sealed_hash {
            let computed = self.hash();
            if stored != computed {
                misses.push(Miss::PackageTampered { stored, computed });
            }
        }

        misses
    }

    /// 门色：无缺件 → 绿；只有提示级 → 黄；其余 → 红。
    pub fn tone(&self, now: u64) -> Tone {
        let misses = self.audit(now);
        if misses.is_empty() {
            return Tone::Green;
        }
        if misses.iter().all(|m| m.is_advisory()) {
            Tone::Yellow
        } else {
            Tone::Red
        }
    }
}

/// 门色。红门不发证据号，黄门发号但标条件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Red,
    Yellow,
    Green,
}

impl Tone {
    pub fn label(self) -> &'static str {
        match self {
            Tone::Red => "红门",
            Tone::Yellow => "黄门",
            Tone::Green => "绿门",
        }
    }

    pub fn allows_evidence(self) -> bool {
        matches!(self, Tone::Yellow | Tone::Green)
    }
}

/// 证据簿。发号单调、不回收：回收会让「EV-E02-0013」指到两个东西。
#[derive(Clone, Debug)]
pub struct EvidenceBook {
    next: u32,
    issued: Vec<(String, bool)>,
}

impl EvidenceBook {
    pub fn new() -> Self {
        Self {
            next: 1,
            issued: Vec::new(),
        }
    }

    /// 发号。`conditional` 为真表示黄门放行（例如 fuzz 报告刚过期）。
    pub fn issue(&mut self, conditional: bool) -> String {
        let id = format!("{}{:04}", EVIDENCE_PREFIX, self.next);
        if self.next < u32::MAX {
            self.next = self.next.wrapping_add(1);
        }
        self.issued.push((id.clone(), conditional));
        id
    }

    pub fn count(&self) -> usize {
        self.issued.len()
    }

    pub fn all(&self) -> &[(String, bool)] {
        &self.issued
    }

    pub fn last(&self) -> Option<&str> {
        self.issued.last().map(|(id, _)| id.as_str())
    }

    pub fn has(&self, id: &str) -> bool {
        self.issued.iter().any(|(i, _)| i == id)
    }
}

impl Default for EvidenceBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 收口结果：门色 + 缺件 +（绿/黄时）证据号。
#[derive(Clone, Debug)]
pub struct CloseReport {
    pub tone: Tone,
    pub misses: Vec<Miss>,
    pub evidence: Option<String>,
}

impl CloseReport {
    /// 读屏清单。红门时第一条就是最该修的那条。
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "E02 收口 · {} · 缺件 {} 条 · 证据 {}",
            self.tone.label(),
            self.misses.len(),
            self.evidence.clone().unwrap_or_else(|| "未发号".to_string())
        ));
        for m in &self.misses {
            out.push(format!(
                "  [{}] {}",
                m.code().code(),
                m.text()
            ));
        }
        out
    }

    pub fn is_green(&self) -> bool {
        matches!(self.tone, Tone::Green) && self.evidence.is_some()
    }
}

/// 执行收口：审计 → 定门色 →（允许时）发号。
///
/// 顺序是刻意的：**先审后发**。反过来就会在红门上留下一个已发出的号。
pub fn close(pkg: &Package, book: &mut EvidenceBook, now: u64) -> CloseReport {
    let misses = pkg.audit(now);
    let tone = if misses.is_empty() {
        Tone::Green
    } else if misses.iter().all(|m| m.is_advisory()) {
        Tone::Yellow
    } else {
        Tone::Red
    };
    let evidence = if tone.allows_evidence() {
        Some(book.issue(matches!(tone, Tone::Yellow)))
    } else {
        None
    };
    CloseReport {
        tone,
        misses,
        evidence,
    }
}

/// 出厂包：20 条全绿、六件齐、两律全、fuzz 新鲜、双签同哈希。
/// 用途有二：自检（样板必须能过自己的门）与夹具（负例从它减一项即得）。
pub fn standard_package(now: u64) -> Package {
    let mut rows = Vec::new();
    for id in GROUP_FIRST..=GROUP_LAST {
        rows.push(ItemRow::green(id));
    }
    let mut pkg = Package {
        rows,
        fuzz: Some(RerunProof {
            families: RerunProof::FAMILIES_REQUIRED,
            cases: 60,
            verdict_green: true,
            finished_at: now.saturating_sub(600),
        }),
        signatures: Vec::new(),
        sealed_hash: None,
    };
    let h = pkg.seal();
    pkg.signatures.push(Signature {
        name: "owner.varix",
        role: Role::Owner,
        package_hash: h,
    });
    pkg.signatures.push(Signature {
        name: "reviewer.varix",
        role: Role::Reviewer,
        package_hash: h,
    });
    pkg
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_000_000;

    #[test]
    fn group_size_matches_id_span() {
        // 20 条与 3421..=3440 必须自洽，否则名录校验永远绿/永远红。
        assert_eq!((GROUP_LAST - GROUP_FIRST + 1) as usize, GROUP_SIZE);
    }

    #[test]
    fn standard_package_closes_green_and_issues_first_evidence() {
        let pkg = standard_package(NOW);
        let mut book = EvidenceBook::new();
        let r = close(&pkg, &mut book, NOW);
        assert!(r.is_green(), "样板必须过自己的门：{:?}", r.lines());
        assert_eq!(r.evidence.as_deref(), Some("EV-E02-0001"));
        assert_eq!(book.count(), 1);
    }

    #[test]
    fn missing_roster_entry_is_red_and_names_the_gap() {
        let mut pkg = standard_package(NOW);
        pkg.rows.retain(|r| r.id != 3433);
        let mut book = EvidenceBook::new();
        let r = close(&pkg, &mut book, NOW);
        assert_eq!(r.tone, Tone::Red);
        assert!(
            r.misses
                .iter()
                .any(|m| matches!(m, Miss::RosterGap { missing: 3433 })),
            "{:?}",
            r.lines()
        );
        assert!(r.evidence.is_none(), "红门不发证据号");
        assert_eq!(book.count(), 0, "未发号的簿必须干净");
    }

    #[test]
    fn red_check_blocks_and_does_not_burn_an_evidence_number() {
        let mut pkg = standard_package(NOW);
        if let Some(row) = pkg.rows.iter_mut().find(|r| r.id == 3425) {
            row.green = false;
        }
        let mut book = EvidenceBook::new();
        let r = close(&pkg, &mut book, NOW);
        assert_eq!(r.tone, Tone::Red);
        assert!(r
            .misses
            .iter()
            .any(|m| matches!(m, Miss::ChecksRed { id: 3425 })));
        // 修好之后号必须还是 0001——红门没有消耗号段。
        let mut fixed = standard_package(NOW);
        fixed.sealed_hash = None;
        let h = fixed.seal();
        for s in fixed.signatures.iter_mut() {
            s.package_hash = h;
        }
        let r2 = close(&fixed, &mut book, NOW);
        assert!(r2.is_green());
        assert_eq!(r2.evidence.as_deref(), Some("EV-E02-0001"));
    }

    #[test]
    fn unproven_law_is_red() {
        let mut pkg = standard_package(NOW);
        for row in pkg.rows.iter_mut() {
            row.laws[Law::MustRollback.index()] = false;
        }
        let mut book = EvidenceBook::new();
        let r = close(&pkg, &mut book, NOW);
        assert_eq!(r.tone, Tone::Red);
        assert!(r.misses.iter().any(|m| matches!(
            m,
            Miss::LawUnproven {
                law: Law::MustRollback,
                ..
            }
        )));
    }

    #[test]
    fn missing_artifact_names_which_one() {
        let mut pkg = standard_package(NOW);
        if let Some(row) = pkg.rows.iter_mut().find(|r| r.id == 3439) {
            row.artifacts = Artifacts::full().with(Artifact::Bench).with(Artifact::Docs);
        }
        // 手工去掉基准位
        if let Some(row) = pkg.rows.iter_mut().find(|r| r.id == 3439) {
            row.artifacts = Artifacts(Artifacts::full().0 & !Artifact::Bench.bit());
        }
        let mut book = EvidenceBook::new();
        let r = close(&pkg, &mut book, NOW);
        assert_eq!(r.tone, Tone::Red);
        assert!(r.misses.iter().any(|m| matches!(
            m,
            Miss::ArtifactMissing {
                id: 3439,
                which: Artifact::Bench
            }
        )));
        assert!(r.lines().iter().any(|l| l.contains("缺基准")));
    }

    #[test]
    fn stale_fuzz_is_yellow_not_green() {
        let mut pkg = standard_package(NOW);
        pkg.fuzz = Some(RerunProof {
            families: 3,
            cases: 60,
            verdict_green: true,
            finished_at: NOW.saturating_sub(FUZZ_FRESH_TICKS * 2),
        });
        let mut book = EvidenceBook::new();
        let r = close(&pkg, &mut book, NOW);
        assert_eq!(r.tone, Tone::Yellow);
        assert!(r.misses.iter().all(|m| m.is_advisory()));
        // 黄门可以放行，但发的是「条件号」。
        let id = r.evidence.clone().unwrap_or_default();
        assert!(id.starts_with(EVIDENCE_PREFIX));
        assert_eq!(book.all().first().map(|(_, c)| *c), Some(true));
    }

    #[test]
    fn fuzz_without_all_families_counts_as_missing() {
        let mut pkg = standard_package(NOW);
        pkg.fuzz = Some(RerunProof {
            families: 2,
            cases: 60,
            verdict_green: true,
            finished_at: NOW,
        });
        let mut book = EvidenceBook::new();
        let r = close(&pkg, &mut book, NOW);
        assert_eq!(r.tone, Tone::Red);
        assert!(r.misses.iter().any(|m| matches!(m, Miss::FuzzMissing)));
    }

    #[test]
    fn double_sign_needs_two_names_two_roles_one_hash() {
        let mut pkg = standard_package(NOW);
        // 同一个人签两次
        if let Some(s) = pkg.signatures.get_mut(1) {
            s.name = "owner.varix";
        }
        let mut book = EvidenceBook::new();
        assert_eq!(close(&pkg, &mut book, NOW).tone, Tone::Red);

        // 同名同角色都合法，只改角色为相同
        let mut pkg2 = standard_package(NOW);
        if let Some(s) = pkg2.signatures.get_mut(1) {
            s.role = Role::Owner;
        }
        let r2 = close(&pkg2, &mut book, NOW);
        assert!(r2.misses.iter().any(|m| matches!(m, Miss::SignerRole)));

        // 签了别的包
        let mut pkg3 = standard_package(NOW);
        if let Some(s) = pkg3.signatures.get_mut(1) {
            s.package_hash ^= 0xFFFF;
        }
        let r3 = close(&pkg3, &mut book, NOW);
        assert!(r3.misses.iter().any(|m| matches!(m, Miss::SignerHash { .. })));
    }

    #[test]
    fn tampering_after_seal_is_detected() {
        let mut pkg = standard_package(NOW);
        let before = pkg.sealed_hash.unwrap_or(0);
        // 偷偷把一条改成不绿（模拟封包后被改）
        if let Some(row) = pkg.rows.iter_mut().find(|r| r.id == 3430) {
            row.diff_archived = false;
        }
        let r = close(&pkg, &mut EvidenceBook::new(), NOW);
        assert_eq!(r.tone, Tone::Red);
        assert!(r.misses.iter().any(|m| {
            matches!(m, Miss::PackageTampered { stored, computed } if *stored == before && *computed != before)
        }));
    }

    #[test]
    fn evidence_numbers_are_monotonic_and_unique() {
        let mut book = EvidenceBook::new();
        let pkg = standard_package(NOW);
        let a = close(&pkg, &mut book, NOW).evidence.unwrap_or_default();
        let b = close(&pkg, &mut book, NOW).evidence.unwrap_or_default();
        assert_eq!(a, "EV-E02-0001");
        assert_eq!(b, "EV-E02-0002");
        assert_ne!(a, b);
        assert!(book.has(&a) && book.has(&b));
        assert_eq!(book.last(), Some("EV-E02-0002"));
    }

    #[test]
    fn artifacts_bitmap_reports_first_missing() {
        let full = Artifacts::full();
        assert!(full.is_full());
        assert_eq!(full.count(), 6);
        let lacking = Artifacts(Artifacts::full().0 & !Artifact::Fuzz.bit());
        assert_eq!(lacking.first_missing(), Some(Artifact::Fuzz));
        assert_eq!(lacking.count(), 5);
        assert!(!lacking.is_full());
    }

    #[test]
    fn audit_reports_everything_not_just_the_first() {
        let mut pkg = standard_package(NOW);
        pkg.sealed_hash = None;
        if let Some(row) = pkg.rows.iter_mut().find(|r| r.id == 3422) {
            row.green = false;
            row.diff_archived = false;
        }
        let misses = pkg.audit(NOW);
        let has_red = misses.iter().any(|m| matches!(m, Miss::ChecksRed { id: 3422 }));
        let has_diff = misses
            .iter()
            .any(|m| matches!(m, Miss::DiffMissing { id: 3422 }));
        assert!(has_red && has_diff, "一次报全：{:?}", misses.len());
    }
}
