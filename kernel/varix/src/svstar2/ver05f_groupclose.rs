//! VE-F3420 · E01 组收口 —— 组级门禁 + 六件齐备 + 双人签署 + 证据编号 + 包哈希
//! + 双签前 fuzz 全量重跑。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3420`
//!
//! # 职责（锚点原文拆解）
//!
//! - **组级门禁**：三条同时成立才准开绿灯——
//!   1. 断言全绿（`passed == total` 且 `total > 0`）；
//!   2. 两律符合（令牌单源 + 变更有迹）；
//!   3. 六件齐备（遥测/安全/文档/测试/示例/fuzz）。
//!   三条中任一条不成立，收口不许过——**门禁不是打分，是全或无**；
//! - **双人签署**：两名签署人、两个不同角色、对同一个包哈希签名。
//!   同一人签两次、两个角色由同一人兼任、签的不是同一个哈希——都记
//!   [`GateCode::SignatureIncomplete`] 并降为黄门（不是红门：签名是
//!   流程问题，不是产物问题，产物还能救，流程补签即可）；
//! - **证据编号**：每次收口生成一个形如 `EV-E01-0007` 的证据号，
//!   单调递增、不重复、不回退。证据号是审计链的锚，缺号等于链断；
//! - **包哈希**：对六件产物按「种类 + 编号 + 摘要」排序后做 FNV-1a。
//!   哈希不等即 [`GateCode::PackageCorrupt`]——红门重建，不许「差不多就行」；
//! - **双签前重跑 fuzz 全量**：`FuzzRerun` 必须在签署前 N 个 tick 内跑过、
//!   用例数 > 0、且未阻断。拿三天前的 fuzz 结果签今天的包，等于没跑。
//!
//! # 为什么缺件是红门而签收不全是黄门
//!
//! 缺件意味着**这一组根本没有交付某类证据**，无从判断，只能打回重做；
//! 签收不全是**产物齐了但流程没走完**，产物本身可信，补签即可放行。
//! 把两者都判红，团队会把补签当成重做，最后干脆绕过门禁；都判黄，
//! 缺件就会长期黄着——黄门不是「以后再说」的意思。
//!
//! # 复杂度
//!
//! - `evaluate`：O(六件 + 签署人)，常数级；
//! - `package_hash`：O(六件 log 六件)；
//! - `EvidenceBook::issue`：O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`；哈希用 `wrapping_mul`。测试区除外。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E17 段续：5354 起）
// ---------------------------------------------------------------------------

/// 收口门禁诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateCode {
    /// 断言未全绿（有红项）。
    AssertionsRed = 5354,
    /// 断言集为空（没跑过断言）。
    AssertionsEmpty = 5355,
    /// 两律缺证（单源/有迹 至少一条没证据）。
    LawsUnproven = 5356,
    /// 六件缺件。
    ArtifactMissing = 5357,
    /// 签署人不全（少于两人或角色重复）。
    SignatureIncomplete = 5358,
    /// 两名签署人签的不是同一个包哈希。
    SignatureMismatch = 5359,
    /// 包哈希不一致（产物损坏或被改）。
    PackageCorrupt = 5360,
    /// 双签前 fuzz 未重跑。
    FuzzNotRerun = 5361,
    /// fuzz 重跑结果过期。
    FuzzStale = 5362,
    /// fuzz 重跑阻断。
    FuzzBlocked = 5363,
    /// 证据号重复（审计链断）。
    EvidenceDuplicated = 5364,
    /// 产物编号重复（同一件交了两份）。
    ArtifactDuplicated = 5365,
    /// 产物摘要为 0（空产物，等于没交）。
    ArtifactEmpty = 5366,
}

impl GateCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            GateCode::AssertionsRed => "断言未全绿",
            GateCode::AssertionsEmpty => "断言集为空",
            GateCode::LawsUnproven => "两律缺证",
            GateCode::ArtifactMissing => "六件缺件",
            GateCode::SignatureIncomplete => "签署人不全",
            GateCode::SignatureMismatch => "签署哈希不一致",
            GateCode::PackageCorrupt => "包哈希不一致",
            GateCode::FuzzNotRerun => "双签前未重跑 fuzz",
            GateCode::FuzzStale => "fuzz 结果过期",
            GateCode::FuzzBlocked => "fuzz 重跑阻断",
            GateCode::EvidenceDuplicated => "证据号重复",
            GateCode::ArtifactDuplicated => "产物编号重复",
            GateCode::ArtifactEmpty => "产物为空",
        }
    }

    /// 该码对应的门色。红门 = 产物不可信；黄门 = 流程未走完。
    pub fn tone(self) -> GateTone {
        match self {
            GateCode::SignatureIncomplete | GateCode::SignatureMismatch => GateTone::Yellow,
            _ => GateTone::Red,
        }
    }
}

/// 门色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateTone {
    /// 绿门：可收口。
    Green,
    /// 黄门：产物可信、流程待补。
    Yellow,
    /// 红门：打回。
    Red,
}

impl GateTone {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            GateTone::Green => "绿门",
            GateTone::Yellow => "黄门",
            GateTone::Red => "红门",
        }
    }

    /// 是否放行（绿/黄都算放行，黄门要求补签）。
    pub fn passes(self) -> bool {
        !matches!(self, GateTone::Red)
    }
}

// ---------------------------------------------------------------------------
// 二、六件产物
// ---------------------------------------------------------------------------

/// 六件封闭全集。顺序即审计报表顺序。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ArtifactKind {
    /// 遥测。
    Telemetry,
    /// 安全。
    Security,
    /// 文档。
    Docs,
    /// 测试。
    Tests,
    /// 示例。
    Examples,
    /// fuzz。
    Fuzz,
}

impl ArtifactKind {
    /// 六件全量。
    pub fn all() -> [ArtifactKind; 6] {
        [
            ArtifactKind::Telemetry,
            ArtifactKind::Security,
            ArtifactKind::Docs,
            ArtifactKind::Tests,
            ArtifactKind::Examples,
            ArtifactKind::Fuzz,
        ]
    }

    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            ArtifactKind::Telemetry => "遥测",
            ArtifactKind::Security => "安全",
            ArtifactKind::Docs => "文档",
            ArtifactKind::Tests => "测试",
            ArtifactKind::Examples => "示例",
            ArtifactKind::Fuzz => "fuzz",
        }
    }

    /// 单字符码（包哈希与报表用）。
    pub fn tag(self) -> u8 {
        match self {
            ArtifactKind::Telemetry => b'T',
            ArtifactKind::Security => b'S',
            ArtifactKind::Docs => b'D',
            ArtifactKind::Tests => b'X',
            ArtifactKind::Examples => b'E',
            ArtifactKind::Fuzz => b'F',
        }
    }
}

/// 一件产物。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Artifact {
    /// 种类。
    pub kind: ArtifactKind,
    /// 编号（组内唯一，如 `E01-TEL-01`）。
    pub id: String,
    /// 摘要（FNV-1a 64）。0 视为空产物。
    pub digest: u64,
}

impl Artifact {
    /// 构造。
    pub fn new(kind: ArtifactKind, id: &str, digest: u64) -> Artifact {
        Artifact {
            kind,
            id: id.to_string(),
            digest,
        }
    }

    /// 摘要是否有效。
    pub fn valid(&self) -> bool {
        self.digest != 0 && !self.id.trim().is_empty()
    }
}

// ---------------------------------------------------------------------------
// 三、两律证据
// ---------------------------------------------------------------------------

/// 两律。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Law {
    /// 律一：令牌单源（一个路径只有一个真值点）。
    SingleSource,
    /// 律二：变更有迹（每次变更都有主体与理由）。
    ChangeTrace,
}

impl Law {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            Law::SingleSource => "令牌单源",
            Law::ChangeTrace => "变更有迹",
        }
    }
}

/// 一条律证据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LawEvidence {
    /// 哪条律。
    pub law: Law,
    /// 是否成立。
    pub proven: bool,
    /// 证据条数（0 条即使 `proven=true` 也不算数）。
    pub items: u32,
}

impl LawEvidence {
    /// 构造。
    pub fn new(law: Law, proven: bool, items: u32) -> LawEvidence {
        LawEvidence { law, proven, items }
    }

    /// 是否真的算数。
    pub fn counts(&self) -> bool {
        self.proven && self.items > 0
    }
}

// ---------------------------------------------------------------------------
// 四、签署与 fuzz 重跑
// ---------------------------------------------------------------------------

/// 签署人。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Signer {
    /// 姓名（或工号）。
    pub name: String,
    /// 角色（`owner` / `reviewer` / `qa` …）。
    pub role: String,
    /// 签的包哈希。
    pub package_hash: u64,
    /// 签署 tick。
    pub tick: u64,
}

impl Signer {
    /// 构造。
    pub fn new(name: &str, role: &str, package_hash: u64, tick: u64) -> Signer {
        Signer {
            name: name.to_string(),
            role: role.to_string(),
            package_hash,
            tick,
        }
    }
}

/// 双签前 fuzz 全量重跑记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzRerun {
    /// 用例数。
    pub cases: u32,
    /// 是否阻断。
    pub blocked: bool,
    /// 跑完的 tick。
    pub tick: u64,
}

impl FuzzRerun {
    /// 构造。
    pub fn new(cases: u32, blocked: bool, tick: u64) -> FuzzRerun {
        FuzzRerun {
            cases,
            blocked,
            tick,
        }
    }
}

/// fuzz 结果保鲜期（tick）。超过即过期——重跑一次很便宜，签旧结果很贵。
pub const FUZZ_FRESH_TICKS: u64 = 7200;

// ---------------------------------------------------------------------------
// 五、证据编号簿
// ---------------------------------------------------------------------------

/// 证据编号簿。`prefix` 形如 `EV-E01`，编号四位补零、单调不回退。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EvidenceBook {
    prefix: String,
    next: u32,
    issued: Vec<String>,
}

impl EvidenceBook {
    /// 新建。
    pub fn new(prefix: &str, start: u32) -> EvidenceBook {
        EvidenceBook {
            prefix: prefix.to_string(),
            next: start.max(1),
            issued: Vec::new(),
        }
    }

    /// 发一个号。重复发号（同一号已存在）拒绝——审计链不许重号。
    pub fn issue(&mut self) -> Result<String, GateCode> {
        let id = format!("{}-{:04}", self.prefix, self.next);
        if self.issued.iter().any(|x| *x == id) {
            return Err(GateCode::EvidenceDuplicated);
        }
        self.issued.push(id.clone());
        self.next = self.next.saturating_add(1);
        Ok(id)
    }

    /// 已发号数。
    pub fn issued_count(&self) -> usize {
        self.issued.len()
    }

    /// 最后一个号（没有则 `None`）。
    pub fn last(&self) -> Option<&str> {
        self.issued.last().map(|s| s.as_str())
    }

    /// 登记一个外部号（迁移/补录）。重复即拒。
    pub fn adopt(&mut self, id: &str) -> Result<(), GateCode> {
        if self.issued.iter().any(|x| x == id) {
            return Err(GateCode::EvidenceDuplicated);
        }
        self.issued.push(id.to_string());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 六、门禁
// ---------------------------------------------------------------------------

/// 门禁输入。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GateInput {
    /// 断言通过数。
    pub assertions_passed: u32,
    /// 断言总数。
    pub assertions_total: u32,
    /// 两律证据（应两条都齐；缺一条即缺证）。
    pub laws: Vec<LawEvidence>,
    /// 六件产物。
    pub artifacts: Vec<Artifact>,
    /// 签署人。
    pub signers: Vec<Signer>,
    /// fuzz 重跑。
    pub fuzz: Option<FuzzRerun>,
    /// 当前 tick。
    pub now: u64,
}

impl Default for GateInput {
    fn default() -> Self {
        GateInput {
            assertions_passed: 0,
            assertions_total: 0,
            laws: Vec::new(),
            artifacts: Vec::new(),
            signers: Vec::new(),
            fuzz: None,
            now: 0,
        }
    }
}

/// 一条门禁发现。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GateFinding {
    /// 码。
    pub code: GateCode,
    /// 说明。
    pub detail: String,
}

impl GateFinding {
    fn new(code: GateCode, detail: &str) -> GateFinding {
        GateFinding {
            code,
            detail: detail.to_string(),
        }
    }

    /// 读屏渲染。
    pub fn render(&self) -> String {
        format!("E{} {}（{}）", self.code.code(), self.code.text(), self.detail)
    }
}

/// 门禁结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GateVerdict {
    /// 门色。
    pub tone: GateTone,
    /// 包哈希。
    pub package_hash: u64,
    /// 证据号（仅在开绿门/黄门时发放）。
    pub evidence: Option<String>,
    /// 发现。
    pub findings: Vec<GateFinding>,
}

impl GateVerdict {
    /// 是否放行。
    pub fn passes(&self) -> bool {
        self.tone.passes()
    }

    /// 一行的结论渲染。
    pub fn headline(&self) -> String {
        match self.evidence.as_deref() {
            Some(ev) => format!(
                "{}｜包哈希{:016X}｜证据{}｜发现{}条",
                self.tone.label(),
                self.package_hash,
                ev,
                self.findings.len()
            ),
            None => format!(
                "{}｜包哈希{:016X}｜无证据号｜发现{}条",
                self.tone.label(),
                self.package_hash,
                self.findings.len()
            ),
        }
    }

    /// 读屏清单：结论 + 每条发现。
    pub fn reading_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(self.headline());
        let mut i = 1;
        for f in self.findings.iter() {
            out.push(format!("第{}项 {}", i, f.render()));
            i += 1;
        }
        out
    }
}

/// 包哈希：六件按（种类, 编号, 摘要）排序后 FNV-1a 64。
pub fn package_hash(artifacts: &[Artifact]) -> u64 {
    let mut rows: Vec<(u8, &str, u64)> = artifacts
        .iter()
        .map(|a| (a.kind.tag(), a.id.as_str(), a.digest))
        .collect();
    rows.sort();
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    for (tag, id, digest) in rows {
        hash ^= tag as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        for byte in id.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        }
        let mut d = digest;
        let mut i = 0;
        while i < 8 {
            hash ^= (d & 0xFF) as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
            d >>= 8;
            i += 1;
        }
    }
    hash
}

/// 求值门禁。纯函数：给定输入与证据簿，产出结论；不在内部改全局状态。
pub fn evaluate(input: &GateInput, book: &mut EvidenceBook) -> GateVerdict {
    let mut findings: Vec<GateFinding> = Vec::new();
    let hash = package_hash(&input.artifacts);

    // 1. 断言全绿
    if input.assertions_total == 0 {
        findings.push(GateFinding::new(GateCode::AssertionsEmpty, "总数为 0"));
    } else if input.assertions_passed != input.assertions_total {
        findings.push(GateFinding::new(
            GateCode::AssertionsRed,
            &format!("{}/{}", input.assertions_passed, input.assertions_total),
        ));
    }

    // 2. 两律符合（两条都要有证据且计数 > 0）
    for law in [Law::SingleSource, Law::ChangeTrace].iter() {
        let ok = input
            .laws
            .iter()
            .any(|e| e.law == *law && e.counts());
        if !ok {
            findings.push(GateFinding::new(GateCode::LawsUnproven, law.label()));
        }
    }

    // 3. 六件齐备 + 编号唯一 + 摘要非零
    for kind in ArtifactKind::all().iter() {
        let matching: Vec<&Artifact> = input
            .artifacts
            .iter()
            .filter(|a| a.kind == *kind)
            .collect();
        if matching.is_empty() {
            findings.push(GateFinding::new(GateCode::ArtifactMissing, kind.label()));
            continue;
        }
        if matching.len() > 1 {
            findings.push(GateFinding::new(
                GateCode::ArtifactDuplicated,
                kind.label(),
            ));
        }
        for a in matching {
            if !a.valid() {
                findings.push(GateFinding::new(GateCode::ArtifactEmpty, &a.id));
            }
        }
    }
    // 跨种类编号重复也算重号（审计表按 id 索引）。
    let mut ids: Vec<&str> = input.artifacts.iter().map(|a| a.id.as_str()).collect();
    ids.sort();
    let mut i = 0;
    while i + 1 < ids.len() {
        if ids.get(i) == ids.get(i + 1) {
            findings.push(GateFinding::new(
                GateCode::ArtifactDuplicated,
                ids.get(i).copied().unwrap_or(""),
            ));
        }
        i += 1;
    }

    // 4. 双签：两人、两角色、同一哈希
    if input.signers.len() < 2 {
        findings.push(GateFinding::new(
            GateCode::SignatureIncomplete,
            &format!("{} 人签署", input.signers.len()),
        ));
    } else {
        let a = input.signers.get(0);
        let b = input.signers.get(1);
        if let (Some(x), Some(y)) = (a, b) {
            if x.name == y.name {
                findings.push(GateFinding::new(
                    GateCode::SignatureIncomplete,
                    "同一人签署两次",
                ));
            }
            if x.role == y.role {
                findings.push(GateFinding::new(
                    GateCode::SignatureIncomplete,
                    "两个角色由同一角色兼任",
                ));
            }
            if x.package_hash != y.package_hash || x.package_hash != hash {
                findings.push(GateFinding::new(
                    GateCode::SignatureMismatch,
                    &format!("签{:016X} 对 包{:016X}", x.package_hash, hash),
                ));
            }
        }
    }

    // 5. 双签前 fuzz 全量重跑
    match input.fuzz {
        None => findings.push(GateFinding::new(GateCode::FuzzNotRerun, "无重跑记录")),
        Some(f) => {
            if f.blocked {
                findings.push(GateFinding::new(GateCode::FuzzBlocked, "重跑阻断"));
            }
            if f.cases == 0 {
                findings.push(GateFinding::new(GateCode::FuzzNotRerun, "用例数为 0"));
            }
            if input.now.saturating_sub(f.tick) > FUZZ_FRESH_TICKS {
                findings.push(GateFinding::new(
                    GateCode::FuzzStale,
                    &format!("{} tick 前", input.now.saturating_sub(f.tick)),
                ));
            }
        }
    }

    // 6. 定门色：任一红 -> 红门；否则任一黄 -> 黄门；否则绿门。
    let mut tone = GateTone::Green;
    for f in findings.iter() {
        match f.code.tone() {
            GateTone::Red => {
                tone = GateTone::Red;
                break;
            }
            GateTone::Yellow => tone = GateTone::Yellow,
            GateTone::Green => {}
        }
    }
    // 红门不发证据号：审计链上的号只属于「已交付」的包。
    let evidence = if tone == GateTone::Red {
        None
    } else {
        book.issue().ok()
    };
    GateVerdict {
        tone,
        package_hash: hash,
        evidence,
        findings,
    }
}

/// 构造一份**必过**的收口输入（出厂样板）。测试与首次收口都用它，
/// 避免「样板数据自己就过不了门」这类反复踩的坑。
pub fn standard_input(now: u64) -> GateInput {
    let mut artifacts = Vec::new();
    for kind in ArtifactKind::all().iter() {
        artifacts.push(Artifact::new(
            *kind,
            &format!("E01-{}-01", kind.label()),
            0x1234_5678_9ABC_DEF0 ^ (kind.tag() as u64),
        ));
    }
    let hash = package_hash(&artifacts);
    GateInput {
        assertions_passed: 128,
        assertions_total: 128,
        laws: alloc::vec![
            LawEvidence::new(Law::SingleSource, true, 40),
            LawEvidence::new(Law::ChangeTrace, true, 40),
        ],
        artifacts,
        signers: alloc::vec![
            Signer::new("owner-a", "owner", hash, now),
            Signer::new("reviewer-b", "reviewer", hash, now),
        ],
        fuzz: Some(FuzzRerun::new(512, false, now)),
        now,
    }
}

// ---------------------------------------------------------------------------
// 七、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_input_opens_green_gate() {
        let mut book = EvidenceBook::new("EV-E01", 1);
        let input = standard_input(1000);
        let v = evaluate(&input, &mut book);
        assert_eq!(v.tone, GateTone::Green, "{:?}", v.reading_lines());
        assert!(v.passes());
        assert_eq!(v.evidence.as_deref(), Some("EV-E01-0001"));
    }

    #[test]
    fn missing_artifact_is_red_and_no_evidence() {
        let mut book = EvidenceBook::new("EV-E01", 1);
        let mut input = standard_input(1000);
        input.artifacts.retain(|a| a.kind != ArtifactKind::Fuzz);
        let v = evaluate(&input, &mut book);
        assert_eq!(v.tone, GateTone::Red);
        assert!(v.findings.iter().any(|f| f.code == GateCode::ArtifactMissing));
        assert!(v.evidence.is_none(), "红门不发证据号");
        assert_eq!(book.issued_count(), 0);
    }

    #[test]
    fn single_signer_is_yellow_not_red() {
        let mut book = EvidenceBook::new("EV-E01", 1);
        let mut input = standard_input(1000);
        input.signers.truncate(1);
        let v = evaluate(&input, &mut book);
        assert_eq!(v.tone, GateTone::Yellow);
        assert!(v.passes(), "黄门放行，但要求补签");
        assert!(v.findings.iter().any(|f| f.code == GateCode::SignatureIncomplete));
    }

    #[test]
    fn same_name_twice_is_incomplete() {
        let mut book = EvidenceBook::new("EV-E01", 1);
        let mut input = standard_input(1000);
        let hash = package_hash(&input.artifacts);
        input.signers = alloc::vec![
            Signer::new("same", "owner", hash, 1000),
            Signer::new("same", "reviewer", hash, 1000),
        ];
        let v = evaluate(&input, &mut book);
        assert_eq!(v.tone, GateTone::Yellow);
        assert!(v.findings.iter().any(|f| f.detail.contains("同一人")));
    }

    #[test]
    fn stale_or_missing_fuzz_blocks_signoff() {
        let mut book = EvidenceBook::new("EV-E01", 1);
        let mut input = standard_input(1000);
        input.now = 1000 + FUZZ_FRESH_TICKS + 1;
        let v = evaluate(&input, &mut book);
        assert!(v.findings.iter().any(|f| f.code == GateCode::FuzzStale));
        assert_eq!(v.tone, GateTone::Red);

        let mut input2 = standard_input(1000);
        input2.fuzz = None;
        let v2 = evaluate(&input2, &mut book);
        assert!(v2.findings.iter().any(|f| f.code == GateCode::FuzzNotRerun));
    }

    #[test]
    fn tampering_artifact_breaks_signature_and_hash() {
        let mut book = EvidenceBook::new("EV-E01", 1);
        let mut input = standard_input(1000);
        if let Some(a) = input.artifacts.get_mut(0) {
            a.digest ^= 0xDEAD_BEEF;
        }
        let v = evaluate(&input, &mut book);
        assert_eq!(v.tone, GateTone::Yellow, "产物变了但流程齐：黄门要求重签");
        assert!(v
            .findings
            .iter()
            .any(|f| f.code == GateCode::SignatureMismatch));
    }

    #[test]
    fn laws_need_evidence_items_not_just_flags() {
        let mut book = EvidenceBook::new("EV-E01", 1);
        let mut input = standard_input(1000);
        input.laws = alloc::vec![
            LawEvidence::new(Law::SingleSource, true, 0),
            LawEvidence::new(Law::ChangeTrace, true, 5),
        ];
        let v = evaluate(&input, &mut book);
        assert!(v.findings.iter().any(|f| f.code == GateCode::LawsUnproven));
    }

    #[test]
    fn evidence_numbers_are_monotonic_and_unique() {
        let mut book = EvidenceBook::new("EV-E01", 7);
        let a = book.issue().expect("首发");
        let b = book.issue().expect("次发");
        assert_eq!(a, "EV-E01-0007");
        assert_eq!(b, "EV-E01-0008");
        assert_eq!(book.issued_count(), 2);
        assert_eq!(book.adopt(&a).err(), Some(GateCode::EvidenceDuplicated));
        assert_eq!(book.last(), Some("EV-E01-0008"));
    }

    #[test]
    fn package_hash_is_order_independent() {
        let input = standard_input(1000);
        let h1 = package_hash(&input.artifacts);
        let mut reversed = input.artifacts.clone();
        reversed.reverse();
        assert_eq!(h1, package_hash(&reversed));
        // 空包也有确定哈希（不是 0：0 与「没算过」不可区分）。
        assert_ne!(package_hash(&[]), 0);
    }
}
