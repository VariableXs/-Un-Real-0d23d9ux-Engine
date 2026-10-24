//! VE-F3680 · R04 组收口双签（VE-S 域 · 批次 R04 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3680`
//!
//! **职责定位（锚点原文）**：R04 组收口双签——
//! - **接口总账**：R04 冻结 API——四域模型 / 生效链 / 场景规则 / 预设画像 / 推荐 /
//!   导出 / 同步 / 消解 / 调试器 / 测试生成器十族签名表（复述）；
//! - **映射断言**：签名表映射无缺；
//! - **20 项自检**；
//! - **联动核验**：覆盖禁令 P0 断言全绿 / 四权可达证据 / 独立解析证据 /
//!   200ms 断言全绿 / 同步关闭网络审计证据。
//!
//! **判据（锚点原文）**：十族签名表、映射断言、联动五证、双签结构、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 映射缺→阻断 | [`C80Code::MAP_MISSING`] |
//! | P0 断言失败→阻断（主权/隐私硬门） | [`C80Code::P0_BLOCK`] |
//! | 独立解析失败→阻断（开放硬门） | [`C80Code::PARSE_BLOCK`] |
//! | 联动分歧→两轮上限（复述） | [`C80Code::LINK_DIVERGE`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3680 诊断码段（独占 0x80xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C80Code(pub u16);

impl C80Code {
    /// 映射缺（阻断）。
    pub const MAP_MISSING: C80Code = C80Code(0x8001);
    /// P0 断言失败（阻断）。
    pub const P0_BLOCK: C80Code = C80Code(0x8002);
    /// 独立解析失败（阻断）。
    pub const PARSE_BLOCK: C80Code = C80Code(0x8003);
    /// 联动分歧（两轮上限）。
    pub const LINK_DIVERGE: C80Code = C80Code(0x8004);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C80Code::MAP_MISSING => "签名映射缺。建议：阻断收口。",
            C80Code::P0_BLOCK => "P0 断言失败（主权/隐私硬门）。建议：阻断。",
            C80Code::PARSE_BLOCK => "独立解析失败（开放硬门）。建议：阻断。",
            C80Code::LINK_DIVERGE => "联动分歧。建议：两轮上限处理。",
            _ => "未知诊断码。",
        }
    }
}

// ── 十族签名表 ──────────────────────────────────────────────────────

/// R04 冻结 API 十族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiFamily {
    /// 四域模型。
    DomainModel,
    /// 生效链。
    ApplyChain,
    /// 场景规则。
    SceneRule,
    /// 预设画像。
    PresetProfile,
    /// 推荐。
    Recommend,
    /// 导出。
    Export,
    /// 同步。
    Sync,
    /// 消解。
    Conflict,
    /// 调试器。
    Debugger,
    /// 测试生成器。
    TestGen,
}

impl ApiFamily {
    pub const ALL: [ApiFamily; 10] = [
        ApiFamily::DomainModel,
        ApiFamily::ApplyChain,
        ApiFamily::SceneRule,
        ApiFamily::PresetProfile,
        ApiFamily::Recommend,
        ApiFamily::Export,
        ApiFamily::Sync,
        ApiFamily::Conflict,
        ApiFamily::Debugger,
        ApiFamily::TestGen,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            ApiFamily::DomainModel => "四域模型",
            ApiFamily::ApplyChain => "生效链",
            ApiFamily::SceneRule => "场景规则",
            ApiFamily::PresetProfile => "预设画像",
            ApiFamily::Recommend => "推荐",
            ApiFamily::Export => "导出",
            ApiFamily::Sync => "同步",
            ApiFamily::Conflict => "消解",
            ApiFamily::Debugger => "调试器",
            ApiFamily::TestGen => "测试生成器",
        }
    }
}

/// 签名表（十族冻结）。
pub struct SignatureTable {
    frozen: Vec<ApiFamily>,
    map_missing: u32,
    last_code: C80Code,
    has_error: bool,
}

impl SignatureTable {
    pub fn new() -> Self {
        Self {
            frozen: Vec::new(),
            map_missing: 0,
            last_code: C80Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C80Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 冻结一族。
    pub fn freeze(&mut self, fam: ApiFamily) {
        if !self.frozen.contains(&fam) {
            self.frozen.push(fam);
        }
    }

    /// 十族齐备。
    pub fn all_frozen(&self) -> bool {
        ApiFamily::ALL.iter().all(|f| self.frozen.contains(f))
    }

    /// 映射断言。
    pub fn assert_mapping(&mut self) -> Result<bool, C80Code> {
        if self.all_frozen() {
            self.last_code = C80Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.map_missing += 1;
            self.last_code = C80Code::MAP_MISSING;
            self.has_error = true;
            Err(C80Code::MAP_MISSING)
        }
    }

    pub fn frozen_count(&self) -> usize {
        self.frozen.len()
    }

    pub fn map_missing(&self) -> u32 {
        self.map_missing
    }
}

// ── 20 项自检 ───────────────────────────────────────────────────────

/// 自检项。
#[derive(Clone, Copy)]
pub struct SelfCheck {
    pub id: u8,
    pub ok: bool,
}

/// 自检组（20 项）。
pub struct SelfCheckSet {
    items: Vec<SelfCheck>,
}

impl SelfCheckSet {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn add(&mut self, id: u8, ok: bool) -> usize {
        self.items.push(SelfCheck { id, ok });
        self.items.len()
    }

    /// 20 项齐备。
    pub fn complete(&self) -> bool {
        self.items.len() >= 20
    }

    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.items.iter().all(|i| i.ok)
    }

    /// 失败项数。
    pub fn fail_count(&self) -> usize {
        self.items.iter().filter(|i| !i.ok).count()
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }
}

// ── 联动五证 ────────────────────────────────────────────────────────

/// 联动证据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkEvidence {
    /// 覆盖禁令 P0 断言全绿。
    CoverBanGreen,
    /// 四权可达证据。
    RightsReachable,
    /// 独立解析证据。
    IndependentParse,
    /// 200ms 断言全绿。
    Apply200Green,
    /// 同步关闭网络审计证据。
    SyncOffAudit,
}

impl LinkEvidence {
    pub const ALL: [LinkEvidence; 5] = [
        LinkEvidence::CoverBanGreen,
        LinkEvidence::RightsReachable,
        LinkEvidence::IndependentParse,
        LinkEvidence::Apply200Green,
        LinkEvidence::SyncOffAudit,
    ];
}

/// 联动核验（P0/解析失败阻断）。
pub struct LinkVerifier {
    evidence: Vec<LinkEvidence>,
    p0_hits: u32,
    parse_hits: u32,
    diverge_hits: u32,
    last_code: C80Code,
    has_error: bool,
}

impl LinkVerifier {
    pub fn new() -> Self {
        Self {
            evidence: Vec::new(),
            p0_hits: 0,
            parse_hits: 0,
            diverge_hits: 0,
            last_code: C80Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C80Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 提交一证（ok=false 视为失败）。
    pub fn submit(&mut self, ev: LinkEvidence, ok: bool) -> Result<bool, C80Code> {
        if ok {
            self.evidence.push(ev);
            self.last_code = C80Code(0);
            self.has_error = false;
            return Ok(true);
        }
        match ev {
            LinkEvidence::CoverBanGreen => {
                self.p0_hits += 1;
                self.last_code = C80Code::P0_BLOCK;
                self.has_error = true;
                Err(C80Code::P0_BLOCK)
            }
            LinkEvidence::IndependentParse => {
                self.parse_hits += 1;
                self.last_code = C80Code::PARSE_BLOCK;
                self.has_error = true;
                Err(C80Code::PARSE_BLOCK)
            }
            _ => {
                self.diverge_hits += 1;
                self.last_code = C80Code::LINK_DIVERGE;
                self.has_error = true;
                Err(C80Code::LINK_DIVERGE)
            }
        }
    }

    /// 五证齐备。
    pub fn all_evidenced(&self) -> bool {
        LinkEvidence::ALL.iter().all(|e| self.evidence.contains(e))
    }

    pub fn evidence_count(&self) -> usize {
        self.evidence.len()
    }

    pub fn p0_hits(&self) -> u32 {
        self.p0_hits
    }

    pub fn parse_hits(&self) -> u32 {
        self.parse_hits
    }

    pub fn diverge_hits(&self) -> u32 {
        self.diverge_hits
    }
}

// ── 双签 ────────────────────────────────────────────────────────────

/// 签名方。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Signer {
    /// 构建方。
    Builder,
    /// 验收方。
    Accepter,
}

/// 双签结构。
pub struct DualSign {
    builder_signed: bool,
    accepter_signed: bool,
}

impl DualSign {
    pub fn new() -> Self {
        Self {
            builder_signed: false,
            accepter_signed: false,
        }
    }

    /// 签名。
    pub fn sign(&mut self, who: Signer) {
        match who {
            Signer::Builder => self.builder_signed = true,
            Signer::Accepter => self.accepter_signed = true,
        }
    }

    /// 双签齐备。
    pub fn both_signed(&self) -> bool {
        self.builder_signed && self.accepter_signed
    }

    pub fn builder_signed(&self) -> bool {
        self.builder_signed
    }

    pub fn accepter_signed(&self) -> bool {
        self.accepter_signed
    }
}

// ── 收口总评 ────────────────────────────────────────────────────────

/// R04 收口。
pub struct R04Close {
    table: SignatureTable,
    checks: SelfCheckSet,
    link: LinkVerifier,
    sign: DualSign,
}

impl R04Close {
    pub fn new() -> Self {
        Self {
            table: SignatureTable::new(),
            checks: SelfCheckSet::new(),
            link: LinkVerifier::new(),
            sign: DualSign::new(),
        }
    }

    pub fn table(&mut self) -> &mut SignatureTable {
        &mut self.table
    }

    pub fn checks(&mut self) -> &mut SelfCheckSet {
        &mut self.checks
    }

    pub fn link(&mut self) -> &mut LinkVerifier {
        &mut self.link
    }

    pub fn sign(&mut self) -> &mut DualSign {
        &mut self.sign
    }

    /// 可收口：映射齐 + 自检全绿 + 五证齐 + 双签。
    pub fn closeable(&self) -> bool {
        self.table.all_frozen()
            && self.checks.complete()
            && self.checks.all_green()
            && self.link.all_evidenced()
            && self.sign.both_signed()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves80_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves80_r04close");
    {
        let t = SignatureTable::new();
        chk(&mut set, "C3680-01", t.frozen_count() == 0 && !t.all_frozen(), "err");
    }
    {
        let mut t = SignatureTable::new();
        for f in ApiFamily::ALL {
            t.freeze(f);
        }
        chk(&mut set, "C3680-02", t.all_frozen() && t.assert_mapping() == Ok(true), "err");
    }
    {
        let mut t = SignatureTable::new();
        t.freeze(ApiFamily::Export);
        let res = t.assert_mapping();
        chk(&mut set, "C3680-03", res.err() == Some(C80Code::MAP_MISSING), "err");
    }
    {
        let mut s = SelfCheckSet::new();
        for i in 0..20u8 {
            s.add(i, true);
        }
        chk(&mut set, "C3680-04", s.complete() && s.all_green(), "err");
    }
    {
        let mut s = SelfCheckSet::new();
        for i in 0..20u8 {
            s.add(i, i != 7);
        }
        chk(&mut set, "C3680-05", s.complete() && !s.all_green() && s.fail_count() == 1, "err");
    }
    {
        let mut l = LinkVerifier::new();
        for e in LinkEvidence::ALL {
            l.submit(e, true);
        }
        chk(&mut set, "C3680-06", l.all_evidenced() && l.evidence_count() == 5, "err");
    }
    {
        let mut l = LinkVerifier::new();
        let res = l.submit(LinkEvidence::CoverBanGreen, false);
        chk(&mut set, "C3680-07", res.err() == Some(C80Code::P0_BLOCK), "err");
    }
    {
        let mut l = LinkVerifier::new();
        let res = l.submit(LinkEvidence::IndependentParse, false);
        chk(&mut set, "C3680-08", res.err() == Some(C80Code::PARSE_BLOCK), "err");
    }
    {
        let mut l = LinkVerifier::new();
        let res = l.submit(LinkEvidence::RightsReachable, false);
        chk(&mut set, "C3680-09", res.err() == Some(C80Code::LINK_DIVERGE), "err");
    }
    {
        let mut d = DualSign::new();
        d.sign(Signer::Builder);
        chk(&mut set, "C3680-10", d.builder_signed() && !d.both_signed(), "err");
    }
    {
        let mut d = DualSign::new();
        d.sign(Signer::Builder);
        d.sign(Signer::Accepter);
        chk(&mut set, "C3680-11", d.both_signed(), "err");
    }
    {
        let mut c = R04Close::new();
        for f in ApiFamily::ALL {
            c.table().freeze(f);
        }
        for i in 0..20u8 {
            c.checks().add(i, true);
        }
        for e in LinkEvidence::ALL {
            c.link().submit(e, true);
        }
        c.sign().sign(Signer::Builder);
        c.sign().sign(Signer::Accepter);
        chk(&mut set, "C3680-12", c.closeable(), "err");
    }
    set
}
