//! VE-F3684 · 社区治理（VE-S 域 · 批次 R05 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3684`
//!
//! **职责定位（锚点原文）**：社区治理——
//! - **行为规范**：创作者/用户行为准则公开；
//! - **违规处置**：广告垃圾/抄袭/恶意/违法四级——分级处置表 + 申诉；
//! - **治理透明**：处罚公开脱敏公告（暗箱处罚 = 公信崩——公示断言）；
//! - **治理独立性**：治理与运营分离（运营 KPI 不得干预治理——干预禁令 P0）。
//!
//! **判据（锚点原文）**：四级处置、公示断言、干预禁令 P0、申诉制度、规范公开、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 暗箱处罚→P1（公示红线实测） | [`C84Code::HIDDEN_PENALTY`] |
//! | 干预检出→P0（红线最高级） | [`C84Code::OPS_INTERFERE`] |
//! | 误罚→申诉复述 | [`C84Code::WRONG_PENALTY`] |
//! | 规范暗改→版本 ADR | [`C84Code::RULE_TAMPER`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3684 诊断码段（独占 0x84xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C84Code(pub u16);

impl C84Code {
    /// 暗箱处罚（P1 公示红线）。
    pub const HIDDEN_PENALTY: C84Code = C84Code(0x8401);
    /// 运营干预治理（P0 红线最高级）。
    pub const OPS_INTERFERE: C84Code = C84Code(0x8402);
    /// 误罚。
    pub const WRONG_PENALTY: C84Code = C84Code(0x8403);
    /// 规范暗改。
    pub const RULE_TAMPER: C84Code = C84Code(0x8404);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C84Code::HIDDEN_PENALTY => "暗箱处罚（P1 公示红线实测）。建议：脱敏公示。",
            C84Code::OPS_INTERFERE => "运营干预治理（P0 红线最高级）。建议：独立性审计。",
            C84Code::WRONG_PENALTY => "误罚。建议：走申诉复议。",
            C84Code::RULE_TAMPER => "规范暗改。建议：版本 ADR 留痕。",
            _ => "未知诊断码。",
        }
    }
}

// ── 违规分级 ────────────────────────────────────────────────────────

/// 违规四级。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ViolationLevel {
    /// 广告垃圾。
    Spam = 0,
    /// 抄袭。
    Plagiarism = 1,
    /// 恶意。
    Malice = 2,
    /// 违法。
    Illegal = 3,
}

impl ViolationLevel {
    pub const ALL: [ViolationLevel; 4] = [
        ViolationLevel::Spam,
        ViolationLevel::Plagiarism,
        ViolationLevel::Malice,
        ViolationLevel::Illegal,
    ];
}

/// 处置动作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Penalty {
    /// 警告。
    Warn,
    /// 限流。
    Throttle,
    /// 下架。
    Takedown,
    /// 封禁。
    Ban,
}

/// 分级处置表。
pub struct PenaltyTable {
    /// (级别, 处置)。
    mapping: Vec<(ViolationLevel, Penalty)>,
}

impl PenaltyTable {
    /// 默认处置表。
    pub fn standard() -> Self {
        Self {
            mapping: vec![
                (ViolationLevel::Spam, Penalty::Warn),
                (ViolationLevel::Plagiarism, Penalty::Takedown),
                (ViolationLevel::Malice, Penalty::Throttle),
                (ViolationLevel::Illegal, Penalty::Ban),
            ],
        }
    }

    /// 查处置。
    pub fn lookup(&self, level: ViolationLevel) -> Option<Penalty> {
        self.mapping
            .iter()
            .find(|(l, _)| *l == level)
            .map(|(_, p)| *p)
    }

    /// 四级齐备。
    pub fn complete(&self) -> bool {
        ViolationLevel::ALL
            .iter()
            .all(|l| self.mapping.iter().any(|(x, _)| x == l))
    }
}

// ── 公示断言 ────────────────────────────────────────────────────────

/// 处罚记录。
#[derive(Clone, Copy)]
pub struct PenaltyRecord {
    pub id: u16,
    pub level: ViolationLevel,
    /// 是否脱敏公示。
    pub published: bool,
}

/// 公示断言器（脱敏合规）。
pub struct PublicNotice {
    records: Vec<PenaltyRecord>,
    hidden_hits: u32,
    last_code: C84Code,
    has_error: bool,
}

impl PublicNotice {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            hidden_hits: 0,
            last_code: C84Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C84Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn record(&mut self, r: PenaltyRecord) -> usize {
        self.records.push(r);
        self.records.len()
    }

    /// 公示审计：每条处罚必须公示。
    pub fn audit(&mut self) -> Result<bool, C84Code> {
        if self.records.iter().all(|r| r.published) {
            self.last_code = C84Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.hidden_hits += 1;
            self.last_code = C84Code::HIDDEN_PENALTY;
            self.has_error = true;
            Err(C84Code::HIDDEN_PENALTY)
        }
    }

    pub fn hidden_hits(&self) -> u32 {
        self.hidden_hits
    }

    pub fn record_count(&self) -> usize {
        self.records.len()
    }
}

// ── 独立性审计 ──────────────────────────────────────────────────────

/// 干预事件。
#[derive(Clone, Copy)]
pub struct InterfereEvent {
    /// 来源（1=运营）。
    pub source: u8,
    /// 是否影响裁决。
    pub affected: bool,
}

/// 独立性审计（干预禁令 P0）。
pub struct IndependenceAudit {
    events: Vec<InterfereEvent>,
    interfere_hits: u32,
    last_code: C84Code,
    has_error: bool,
}

impl IndependenceAudit {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            interfere_hits: 0,
            last_code: C84Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C84Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn ingest(&mut self, e: InterfereEvent) {
        self.events.push(e);
    }

    /// 干预检测：运营影响裁决即 P0。
    pub fn audit(&mut self) -> Result<bool, C84Code> {
        let mut ok = true;
        for e in &self.events {
            if e.source == 1 && e.affected {
                ok = false;
                break;
            }
        }
        if ok {
            self.last_code = C84Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.interfere_hits += 1;
            self.last_code = C84Code::OPS_INTERFERE;
            self.has_error = true;
            Err(C84Code::OPS_INTERFERE)
        }
    }

    pub fn interfere_hits(&self) -> u32 {
        self.interfere_hits
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}

// ── 申诉制度 ────────────────────────────────────────────────────────

/// 申诉单。
#[derive(Clone, Copy)]
pub struct Appeal {
    pub id: u16,
    /// 是否误罚确认。
    pub wrong: bool,
    /// 是否已复议。
    pub reviewed: bool,
}

/// 申诉处理器。
pub struct AppealDesk {
    appeals: Vec<Appeal>,
    wrong_hits: u32,
    last_code: C84Code,
    has_error: bool,
}

impl AppealDesk {
    pub fn new() -> Self {
        Self {
            appeals: Vec::new(),
            wrong_hits: 0,
            last_code: C84Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C84Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn submit(&mut self, a: Appeal) -> usize {
        self.appeals.push(a);
        self.appeals.len()
    }

    /// 复议审计：误罚必须已复议。
    pub fn audit(&mut self) -> Result<bool, C84Code> {
        let mut ok = true;
        for a in &self.appeals {
            if a.wrong && !a.reviewed {
                ok = false;
                break;
            }
        }
        if ok {
            self.last_code = C84Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.wrong_hits += 1;
            self.last_code = C84Code::WRONG_PENALTY;
            self.has_error = true;
            Err(C84Code::WRONG_PENALTY)
        }
    }

    pub fn wrong_hits(&self) -> u32 {
        self.wrong_hits
    }

    pub fn appeal_count(&self) -> usize {
        self.appeals.len()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves84_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves84_comgov");
    {
        let t = PenaltyTable::standard();
        chk(&mut set, "C3684-01", t.complete(), "err");
    }
    {
        let t = PenaltyTable::standard();
        chk(
            &mut set,
            "C3684-02",
            t.lookup(ViolationLevel::Illegal) == Some(Penalty::Ban),
            "err",
        );
    }
    {
        let t = PenaltyTable::standard();
        chk(
            &mut set,
            "C3684-03",
            t.lookup(ViolationLevel::Spam) == Some(Penalty::Warn),
            "err",
        );
    }
    {
        let mut n = PublicNotice::new();
        n.record(PenaltyRecord { id: 1, level: ViolationLevel::Spam, published: true });
        chk(&mut set, "C3684-04", n.audit() == Ok(true), "err");
    }
    {
        let mut n = PublicNotice::new();
        n.record(PenaltyRecord { id: 1, level: ViolationLevel::Malice, published: false });
        let res = n.audit();
        chk(&mut set, "C3684-05", res.err() == Some(C84Code::HIDDEN_PENALTY), "err");
    }
    {
        let mut a = IndependenceAudit::new();
        a.ingest(InterfereEvent { source: 2, affected: true });
        chk(&mut set, "C3684-06", a.audit() == Ok(true), "err");
    }
    {
        let mut a = IndependenceAudit::new();
        a.ingest(InterfereEvent { source: 1, affected: true });
        let res = a.audit();
        chk(&mut set, "C3684-07", res.err() == Some(C84Code::OPS_INTERFERE), "err");
    }
    {
        let mut a = IndependenceAudit::new();
        a.ingest(InterfereEvent { source: 1, affected: false });
        chk(&mut set, "C3684-08", a.audit() == Ok(true), "err");
    }
    {
        let mut d = AppealDesk::new();
        d.submit(Appeal { id: 1, wrong: true, reviewed: true });
        chk(&mut set, "C3684-09", d.audit() == Ok(true), "err");
    }
    {
        let mut d = AppealDesk::new();
        d.submit(Appeal { id: 1, wrong: true, reviewed: false });
        let res = d.audit();
        chk(&mut set, "C3684-10", res.err() == Some(C84Code::WRONG_PENALTY), "err");
    }
    {
        let mut d = AppealDesk::new();
        d.submit(Appeal { id: 1, wrong: false, reviewed: false });
        chk(&mut set, "C3684-11", d.audit() == Ok(true), "err");
    }
    {
        let lv_ok = ViolationLevel::ALL.len() == 4;
        chk(&mut set, "C3684-12", lv_ok, "err");
    }
    set
}
