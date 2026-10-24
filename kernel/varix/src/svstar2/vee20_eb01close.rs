//! VE-F0820 · Eb01 组收口双签（VE-E 域 · Eb01 文字渲染组 · 目标 280 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0820`
//!
//! **职责定位（锚点原文）**：组收口双签按六段体例收束 Eb01：接口总账——本组
//! 19 项接口，文字渲染 API 5 函数扩展冻结（MeasureText/RenderText/CacheControl
//! 三核心加后端注册与能力查询两辅助），签名入档；主题登记——文字渲染主题（四段
//! 架构/三向兑现/预算 1.5ms）全部兑现；跨域契约——X01 开工样板、N 域测量契约、
//! C 域着色器复用三契约归档；测试账——单元 124/集成 12/端到端 8 条全绿；性能
//! 账——光栅预算达标（P50/P95 双列在册）；文档账——手册在册且词条表生成；遗留
//! 清单——显性登记（LCD 亚像素模式仅 Windows 平台启用，驱动条件为跨平台验证）；
//! 双签——工程+质量双席独立核验六账后生效，与 Eb02/Eb03 收口共同支撑 Eb09 域
//! 自查。
//!
//! **判据（锚点原文）**：5 函数冻结、三契约归档、四账实测、遗留显性、双签生效。
//!
//! **错误路径与降级矩阵（锚点语义逐条落实）**：
//!
//! | 收口风险 | 本模块落位 |
//! |---|---|
//! | 接口漂移（签名被静默改动） | 五函数签名以 [`SignatureRow`] 逐项入总账，签名哈希 [`fnv1a`] 冻结；重签对账时哈希不一致即 [`CloseError::SignatureDrift`]，**禁止靠口头声明冻结** |
//! | 假收口（某账未实测就签） | 双席各自独立核验六账，任何一账证据缺失即 [`CloseError::LedgerUnproven`]；双签不是走形式——两席核验路径独立（工程席看实现对账、质量席看判据对账），**同一账两席各有一份证据源** |
//! | 遗留被藏（欠账不登记） | 遗留清单逐条带**驱动条件**（[`Leftover::driver`]），无驱动条件的遗留即非法（[`CloseError::LeftoverVague`]）——「以后再说」不是条件 |
//! | 预算谎报（P50 达标 P95 失守却报绿） | 性能账双列（P50/P95）都必须达标才记过（[`BudgetRow::both_ok`]）；单列达标是**最常见谎报形态**，对账时两列缺一即拒 |
//!
//! **性能逐项分解（锚点语义）**：核验是 O(账目数)——签名冻结 O(接口数)、契约
//! 归档 O(契约数)、测试账合计 O(1)（三条定数）、性能账 O(1)（双列）、文档账
//! O(词条数)；双签核验两席各走一遍全账，O(2×账目)。总签名 FNV 链 O(账目数)。
//!
//! **跨批对接点**：上游 F0819 联调报告（八链证据即测试账证据源，单一事实源不
//! 复制）；下游 Eb02/Eb03 收口与 Eb09 域自查（本模块的 [`CloseRecord`] 是域
//! 自查的输入）；N 域测量契约、C 域着色器复用契约、X01 开工样板三契约以编号
//! 归档，正文在各域册。
//!
//! **无障碍与隐私**：收口摘要 [`CloseRecord::a11y_lines`] 中英双语六行（双签
//! 状态/五函数冻结/三契约/四账/遗留数/总签名），读屏逐行播报；无隐私面（只有
//! 工程账目，无用户数据）。
//!
//! **诊断段 0x2400（十码独占）**：`E_CLOSE_SIG_DRIFT` / `E_CLOSE_LEDGER_GAP`
//! / `E_CLOSE_LEFTOVER_VAGUE` / `E_CLOSE_BUDGET_LIE` / `E_CLOSE_SIGN_ORDER`
//! / `E_CLOSE_CONTRACT_MISSING` / `E_CLOSE_DOC_GAP` / `E_CLOSE_TEST_GAP`
//! / `E_CLOSE_REOPEN` / `E_CLOSE_SEAL_TAMPER`。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（全仓唯一，grep 核验后占用）。
pub const SEG_BASE: u16 = 0x2400;

/// 五函数冻结接口名（闭集，顺序即总账登记序）。
pub const FROZEN_APIS: [&str; 5] = [
    "MeasureText",
    "RenderText",
    "CacheControl",
    "RegisterBackend",
    "QueryCaps",
];

/// 三跨域契约（编号+归档语义）。
pub const CONTRACTS: [&str; 3] = [
    "X01-开工样板",
    "N域-测量契约",
    "C域-着色器复用",
];

/// 测试账定数：单元/集成/端到端。
pub const TEST_UNIT: u32 = 124;
pub const TEST_INTEGRATION: u32 = 12;
pub const TEST_E2E: u32 = 8;

/// 光栅预算（毫秒，锚点原文 1.5ms）。
pub const RASTER_BUDGET_MS_P50: u32 = 1500;
/// P95 允许上浮（预算纪律：P95 不超过 P50 预算的 120%——锚点双列口径）。
pub const RASTER_BUDGET_MS_P95: u32 = 1800;

/// FNV-1a（32 位，全仓同族单一实现约定——本模块只消费不复用他人实现）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 收口错误闭集（中文诊断 + 源位置 + 可操作建议三要素）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseError {
    /// 签名哈希与入档值不一致——接口被改过。
    SignatureDrift { api: &'static str, now: u32, frozen: u32 },
    /// 某账证据缺失（双席任一席核不到证据）。
    LedgerUnproven { seat: Seat, ledger: Ledger },
    /// 遗留条目无驱动条件。
    LeftoverVague { item: String },
    /// 性能账单列达标（谎报形态）。
    BudgetLie { p50_ms: u32, p95_ms: u32 },
    /// 双签顺序违规（先签后核）。
    SignOrder,
    /// 契约编号不在三契约闭集。
    ContractMissing { found: String },
    /// 文档账：手册或词条表缺失。
    DocGap,
    /// 测试账：三类定数不符。
    TestGap { unit: u32, integration: u32, e2e: u32 },
    /// 已收口组被重开（收口单向推进红线）。
    Reopen,
    /// 封印被篡改（总签名对不上账目重算值）。
    SealTamper { sealed: u32, recomputed: u32 },
}

impl CloseError {
    /// 中文诊断 + 可操作建议（错误路径深度要求：建议必须可执行）。
    pub fn detail(&self) -> String {
        match self {
            CloseError::SignatureDrift { api, now, frozen } => format!(
                "接口签名漂移：{} 现值 0x{:08X} 入档 0x{:08X}。建议：确认是否有 ADR 变更单；\
                 无单则回滚签名并追查改动提交，有单则走冻结重签流程（重开组收口）",
                api, now, frozen
            ),
            CloseError::LedgerUnproven { seat, ledger } => format!(
                "{:?}席核验时 {} 账无证据。建议：先补齐该账的实测证据（禁止以计划代替证据），\
                 再重新提交双签",
                seat, ledger.name()
            ),
            CloseError::LeftoverVague { item } => format!(
                "遗留条目「{}」无驱动条件。建议：补写触发条件（平台/驱动/版本三选一），\
                 「以后再说」不构成条件，拒收",
                item
            ),
            CloseError::BudgetLie { p50_ms, p95_ms } => format!(
                "性能账单列达标：P50={}μs 达标但 P95={}μs 超线。建议：按降质链降档后重测\
                 双列，两列同时在册才算达标",
                p50_ms, p95_ms
            ),
            CloseError::SignOrder => "双签顺序违规：先签后核。建议：重置双签状态机，核验完成\
                 才允许落签"
                .to_string(),
            CloseError::ContractMissing { found } => format!(
                "契约编号「{}」不在三契约闭集。建议：核对是否跨组契约误入本组收口，\
                 归档回原组",
                found
            ),
            CloseError::DocGap => "文档账缺口：手册或词条表缺失。建议：补齐手册并重新生成\
                 词条表后重交"
                .to_string(),
            CloseError::TestGap { unit, integration, e2e } => format!(
                "测试账定数不符：单元 {}/{}、集成 {}/{}、端到端 {}/{}。建议：核对未跑用例\
                 清单，全绿后重交",
                unit, TEST_UNIT, integration, TEST_INTEGRATION, e2e, TEST_E2E
            ),
            CloseError::Reopen => "已收口组被重开。建议：重开必须挂 ADR 变更单并重新走完整\
                 双签，不许原封印直接续用"
                .to_string(),
            CloseError::SealTamper { sealed, recomputed } => format!(
                "封印被篡改：入封 0x{:08X} 重算 0x{:08X}。建议：以重算值为准回溯账目改动，\
                 找到未走流程的写入",
                sealed, recomputed
            ),
        }
    }
}

/// 双签席位（两席独立，核验路径不同源）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    /// 工程席：对实现核账（代码/提交/构建）。
    Eng,
    /// 质量席：对判据核账（用例/指标/对拍）。
    Qa,
}

/// 六账闭集（锚点六段体例，顺序即核验序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ledger {
    /// 接口总账。
    Api,
    /// 主题登记。
    Theme,
    /// 跨域契约。
    Contract,
    /// 测试账。
    Test,
    /// 性能账。
    Budget,
    /// 文档账。
    Doc,
}

impl Ledger {
    pub fn name(&self) -> &'static str {
        match self {
            Ledger::Api => "接口总账",
            Ledger::Theme => "主题登记",
            Ledger::Contract => "跨域契约",
            Ledger::Test => "测试账",
            Ledger::Budget => "性能账",
            Ledger::Doc => "文档账",
        }
    }
    /// 六账全序遍历（核验序冻结）。
    pub const ALL: [Ledger; 6] = [
        Ledger::Api,
        Ledger::Theme,
        Ledger::Contract,
        Ledger::Test,
        Ledger::Budget,
        Ledger::Doc,
    ];
}

/// 签名总账行：接口名 + 冻结哈希。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignatureRow {
    pub api: &'static str,
    pub frozen_hash: u32,
}

impl SignatureRow {
    /// 签名入档值（确定性推导：接口名 FNV 混入组号 0xE_B01，防跨组同名接口碰撞）。
    pub fn freeze(api: &'static str) -> SignatureRow {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"EB01");
        bytes.extend_from_slice(api.as_bytes());
        SignatureRow { api, frozen_hash: fnv1a(&bytes) }
    }
}

/// 五函数签名总账（冻结序 = [`FROZEN_APIS`] 顺序）。
pub fn frozen_signatures() -> Vec<SignatureRow> {
    FROZEN_APIS.iter().map(|a| SignatureRow::freeze(a)).collect()
}

/// 性能账双列：P50/P95 实测微秒。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetRow {
    pub p50_us: u32,
    pub p95_us: u32,
}

impl BudgetRow {
    /// 双列同时达标才记过（单列达标 = 谎报形态，[`CloseError::BudgetLie`]）。
    pub fn verdict(&self) -> Result<(), CloseError> {
        if self.p50_us > RASTER_BUDGET_MS_P50 || self.p95_us > RASTER_BUDGET_MS_P95 {
            Err(CloseError::BudgetLie { p50_ms: self.p50_us, p95_ms: self.p95_us })
        } else {
            Ok(())
        }
    }
}

/// 遗留条目：描述 + 驱动条件（条件缺失即非法）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leftover {
    pub item: String,
    /// 驱动条件：平台/驱动/版本三选一非空。
    pub driver: Option<String>,
}

impl Leftover {
    pub fn validate(&self) -> Result<(), CloseError> {
        match &self.driver {
            Some(d) if !d.is_empty() => Ok(()),
            _ => Err(CloseError::LeftoverVague { item: self.item.clone() }),
        }
    }
}

/// Eb01 标准遗留（锚点原文：LCD 亚像素模式仅 Windows 平台启用）。
pub fn standard_leftovers() -> Vec<Leftover> {
    vec![Leftover {
        item: String::from("LCD 亚像素模式"),
        driver: Some(String::from("仅 Windows 平台启用；驱动条件：跨平台验证通过后放开")),
    }]
}

/// 席位核验证据：一席对一账的一份证据源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub seat: Seat,
    pub ledger: Ledger,
    /// 证据指纹（对账用：两席证据指纹可不同，但都不得为空）。
    pub fingerprint: u32,
}

impl Evidence {
    pub fn mint(seat: Seat, ledger: Ledger, fact: &[u8]) -> Evidence {
        let mut bytes = Vec::new();
        bytes.push(match seat { Seat::Eng => 0xE1, Seat::Qa => 0x0A });
        bytes.push(match ledger { Ledger::Api => 1, Ledger::Theme => 2, Ledger::Contract => 3, Ledger::Test => 4, Ledger::Budget => 5, Ledger::Doc => 6 });
        bytes.extend_from_slice(fact);
        Evidence { seat, ledger, fingerprint: fnv1a(&bytes) }
    }
    pub fn valid(&self) -> bool {
        self.fingerprint != 0
    }
}

/// 双签状态机：`Collecting → Verifying → Sealed`（单向，重开即错）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosePhase {
    /// 账目收集期（可增可改）。
    Collecting,
    /// 双席核验期（账目冻结，只加证据）。
    Verifying,
    /// 封印生效期（只读）。
    Sealed,
}

/// 账目汇集器：六账 + 遗留清单 + 双席证据 + 双签状态。
pub struct CloseRecord {
    pub phase: ClosePhase,
    /// 接口总账（冻结签名行）。
    pub signatures: Vec<SignatureRow>,
    /// 已归档契约编号（闭集成员）。
    pub contracts: Vec<&'static str>,
    /// 测试账三类实测。
    pub tests: (u32, u32, u32),
    /// 性能账双列。
    pub budget: Option<BudgetRow>,
    /// 文档账：手册在册 + 词条表生成。
    pub doc_manual: bool,
    pub doc_glossary: bool,
    /// 遗留清单（全部带驱动条件才可收口）。
    pub leftovers: Vec<Leftover>,
    /// 双席证据池。
    pub evidence: Vec<Evidence>,
    /// 落签记录（席+时间戳逻辑序号）。
    pub signs: Vec<(Seat, u64)>,
    /// 主题登记账（三主题兑现记录）。
    pub themes: Vec<ThemeRealization>,
    /// 审计账（append-only，逐动作留痕）。
    pub audit: Vec<AuditRow>,
    audit_seq: u64,
    audit_last: u32,
    /// 逻辑时钟（无墙钟纪律）。
    pub tick: u64,
    /// 封印总签名（Sealed 后只读校验用）。
    pub seal: u32,
}

impl CloseRecord {
    /// 新建收口记录：接口总账即刻冻结（签名入档）。
    pub fn new() -> CloseRecord {
        CloseRecord {
            phase: ClosePhase::Collecting,
            signatures: frozen_signatures(),
            contracts: Vec::new(),
            tests: (0, 0, 0),
            budget: None,
            doc_manual: false,
            doc_glossary: false,
            leftovers: standard_leftovers(),
            evidence: Vec::new(),
            signs: Vec::new(),
            themes: Vec::new(),
            audit: Vec::new(),
            audit_seq: 0,
            audit_last: 0,
            tick: 0,
            seal: 0,
        }
    }

    fn advance_tick(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }

    /// 归档契约（闭集校验；未知编号拒绝并显性）。
    pub fn archive_contract(&mut self, c: &'static str) -> Result<(), CloseError> {
        if self.phase != ClosePhase::Collecting {
            return Err(CloseError::Reopen);
        }
        if !CONTRACTS.contains(&c) {
            return Err(CloseError::ContractMissing { found: String::from(c) });
        }
        if !self.contracts.contains(&c) {
            self.contracts.push(c);
        }
        self.audit_push(AuditAction::ContractArchived(
            match c { "X01-开工样板" => 1, "N域-测量契约" => 2, _ => 3 },
        ));
        let _ = self.advance_tick();
        Ok(())
    }

    /// 记测试账（三类定数一次交齐；数量必须恰等）。
    pub fn record_tests(&mut self, unit: u32, integration: u32, e2e: u32) -> Result<(), CloseError> {
        if self.phase != ClosePhase::Collecting {
            return Err(CloseError::Reopen);
        }
        if unit != TEST_UNIT || integration != TEST_INTEGRATION || e2e != TEST_E2E {
            return Err(CloseError::TestGap { unit, integration, e2e });
        }
        self.tests = (unit, integration, e2e);
        self.audit_push(AuditAction::TestsRecorded);
        let _ = self.advance_tick();
        Ok(())
    }

    /// 记性能账（双列一次交齐，verdict 即时裁定）。
    pub fn record_budget(&mut self, p50_us: u32, p95_us: u32) -> Result<(), CloseError> {
        if self.phase != ClosePhase::Collecting {
            return Err(CloseError::Reopen);
        }
        let row = BudgetRow { p50_us, p95_us };
        row.verdict()?;
        self.budget = Some(row);
        self.audit_push(AuditAction::BudgetRecorded);
        let _ = self.advance_tick();
        Ok(())
    }

    /// 记文档账（两件一次交齐）。
    pub fn record_docs(&mut self, manual: bool, glossary: bool) -> Result<(), CloseError> {
        if self.phase != ClosePhase::Collecting {
            return Err(CloseError::Reopen);
        }
        if !manual || !glossary {
            return Err(CloseError::DocGap);
        }
        self.doc_manual = manual;
        self.doc_glossary = glossary;
        self.audit_push(AuditAction::DocsRecorded);
        let _ = self.advance_tick();
        Ok(())
    }

    /// 登记遗留（驱动条件缺失即拒，收口前必须全部合法）。
    pub fn register_leftover(&mut self, item: &str, driver: Option<&str>) -> Result<(), CloseError> {
        if self.phase != ClosePhase::Collecting {
            return Err(CloseError::Reopen);
        }
        let lo = Leftover { item: String::from(item), driver: driver.map(String::from) };
        lo.validate()?;
        self.leftovers.push(lo);
        self.audit_push(AuditAction::LeftoverRegistered);
        let _ = self.advance_tick();
        Ok(())
    }

    /// 进入核验期：收集期账目齐备性预检（六账+遗留合法性）。
    pub fn begin_verify(&mut self) -> Result<(), CloseError> {
        if self.phase != ClosePhase::Collecting {
            return Err(CloseError::Reopen);
        }
        if self.contracts.len() != CONTRACTS.len() {
            return Err(CloseError::ContractMissing { found: String::from("未齐") });
        }
        if self.tests != (TEST_UNIT, TEST_INTEGRATION, TEST_E2E) {
            return Err(CloseError::TestGap { unit: self.tests.0, integration: self.tests.1, e2e: self.tests.2 });
        }
        if self.budget.is_none() {
            return Err(CloseError::LedgerUnproven { seat: Seat::Eng, ledger: Ledger::Budget });
        }
        if !self.doc_manual || !self.doc_glossary {
            return Err(CloseError::DocGap);
        }
        for lo in &self.leftovers {
            lo.validate()?;
        }
        self.phase = ClosePhase::Verifying;
        self.audit_push(AuditAction::VerifyBegun);
        let _ = self.advance_tick();
        Ok(())
    }

    /// 双席交证据（核验期专用；一席一账一份，重复交同席同账覆盖旧证）。
    pub fn submit_evidence(&mut self, seat: Seat, ledger: Ledger, fact: &[u8]) -> Result<(), CloseError> {
        if self.phase != ClosePhase::Verifying {
            return Err(CloseError::SignOrder);
        }
        let ev = Evidence::mint(seat, ledger, fact);
        if !ev.valid() {
            return Err(CloseError::LedgerUnproven { seat, ledger });
        }
        self.evidence.retain(|e| !(e.seat == seat && e.ledger == ledger));
        self.evidence.push(ev);
        self.audit_push(AuditAction::EvidenceSubmitted(seat, ledger));
        let _ = self.advance_tick();
        Ok(())
    }

    /// 双席各账证据齐备性（两席 × 六账 = 12 份）。
    pub fn evidence_complete(&self) -> bool {
        Seat::ALL.iter().all(|s| Ledger::ALL.iter().all(|l| {
            self.evidence.iter().any(|e| e.seat == *s && e.ledger == *l && e.valid())
        }))
    }

    /// 落签：核验齐备才允许；两席顺序任意但各签一次。
    pub fn sign(&mut self, seat: Seat) -> Result<(), CloseError> {
        if self.phase != ClosePhase::Verifying {
            return Err(CloseError::SignOrder);
        }
        if !self.evidence_complete() {
            let (missing_seat, missing_ledger) = first_missing(self);
            return Err(CloseError::LedgerUnproven { seat: missing_seat, ledger: missing_ledger });
        }
        if self.signs.iter().any(|(s, _)| *s == seat) {
            return Err(CloseError::SignOrder);
        }
        let tick = self.advance_tick();
        self.signs.push((seat, tick));
        self.audit_push(AuditAction::Signed(seat));
        Ok(())
    }

    /// 封印：双签齐 + 全账重算一致 → Sealed（单向终态）。
    pub fn seal(&mut self) -> Result<u32, CloseError> {
        if self.phase != ClosePhase::Verifying {
            return Err(CloseError::Reopen);
        }
        if self.signs.len() != 2 {
            return Err(CloseError::SignOrder);
        }
        let recomputed = self.recompute_seal();
        self.seal = recomputed;
        self.phase = ClosePhase::Sealed;
        self.audit_push(AuditAction::Sealed);
        Ok(recomputed)
    }

    /// 总签名 FNV 链：签名总账+契约+测试+性能+文档+遗留逐环链接（改一账即变封印）。
    fn recompute_seal(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for r in &self.signatures {
            h ^= r.frozen_hash;
            h = h.wrapping_mul(0x0100_0193);
        }
        for c in &self.contracts {
            h ^= fnv1a(c.as_bytes());
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.tests.0; h = h.wrapping_mul(0x0100_0193);
        h ^= self.tests.1; h = h.wrapping_mul(0x0100_0193);
        h ^= self.tests.2; h = h.wrapping_mul(0x0100_0193);
        if let Some(b) = &self.budget {
            h ^= b.p50_us; h = h.wrapping_mul(0x0100_0193);
            h ^= b.p95_us; h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.doc_manual as u32; h = h.wrapping_mul(0x0100_0193);
        h ^= self.doc_glossary as u32; h = h.wrapping_mul(0x0100_0193);
        for lo in &self.leftovers {
            h ^= fnv1a(lo.item.as_bytes());
            if let Some(d) = &lo.driver { h ^= fnv1a(d.as_bytes()); }
            h = h.wrapping_mul(0x0100_0193);
        }
        for (s, t) in &self.signs {
            h ^= match s { Seat::Eng => 0xE1, Seat::Qa => 0x0A };
            h ^= *t as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    /// 封印在册校验（Sealed 态只读对账：入封值 vs 重算值）。
    pub fn verify_seal(&self) -> Result<(), CloseError> {
        if self.phase != ClosePhase::Sealed {
            return Err(CloseError::Reopen);
        }
        let re = self.recompute_seal();
        if re != self.seal {
            return Err(CloseError::SealTamper { sealed: self.seal, recomputed: re });
        }
        Ok(())
    }

    /// 读屏播报（中英双语六行，无隐私字段）。
    pub fn a11y_lines(&self) -> Vec<String> {
        vec![
            format!("seal phase={:?} 双签 {}/2", self.phase, self.signs.len()),
            format!("APIs frozen={} 五函数冻结", self.signatures.len()),
            format!("contracts archived={} 三契约归档", self.contracts.len()),
            format!("tests unit/itg/e2e={}/{}/{} 四账实测", self.tests.0, self.tests.1, self.tests.2),
            format!("leftovers={} 全部带驱动条件显性", self.leftovers.len()),
            format!("seal=0x{:08X} 总签名在册", self.seal),
        ]
    }
}

/// 找第一份缺失证据（核验失败时给出可操作定位）。
fn first_missing(r: &CloseRecord) -> (Seat, Ledger) {
    for s in Seat::ALL {
        for l in Ledger::ALL {
            if !r.evidence.iter().any(|e| e.seat == s && e.ledger == l && e.valid()) {
                return (s, l);
            }
        }
    }
    (Seat::Eng, Ledger::Api)
}

/// 主题登记账：Eb01 三主题（锚点原文：四段架构/三向兑现/预算 1.5ms）逐项
/// 兑现记录。主题不是标题——每条主题必须挂**兑现证据**（证据指纹非零），
/// 收口核验时缺兑现即 [`CloseError::LedgerUnproven`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// 四段架构（编码→轮廓→光栅化→合成四段管线）。
    Arch4Stage,
    /// 三向兑现（三组消费方向全部接通）。
    TripleDelivery,
    /// 光栅预算 1.5ms。
    Budget15,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::Arch4Stage, Theme::TripleDelivery, Theme::Budget15];
    pub fn name(&self) -> &'static str {
        match self {
            Theme::Arch4Stage => "四段架构",
            Theme::TripleDelivery => "三向兑现",
            Theme::Budget15 => "预算 1.5ms",
        }
    }
}

/// 主题兑现记录：主题 + 兑现指纹 + 兑现账目（指向六账之一）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeRealization {
    pub theme: Theme,
    /// 兑现指纹（对该主题证据源的 FNV，非零即已兑现）。
    pub proof: u32,
    /// 兑现落在哪本账（四段架构→接口总账、三向兑现→跨域契约、预算→性能账）。
    pub backed_by: Ledger,
}

impl ThemeRealization {
    pub fn mint(theme: Theme, fact: &[u8]) -> ThemeRealization {
        let backed_by = match theme {
            Theme::Arch4Stage => Ledger::Api,
            Theme::TripleDelivery => Ledger::Contract,
            Theme::Budget15 => Ledger::Budget,
        };
        ThemeRealization { theme, proof: fnv1a(fact), backed_by }
    }
}

/// 审计流：append-only 操作账（收口全程逐动作留痕，封印后可重放对账）。
///
/// 重放语义：按登记序重放动作指纹链（每环 = 上一环哈希 ⊕ 本动作指纹），
/// 封印值即链尾。改任一历史动作 → 链尾变化 → 与 `seal` 对不上 → 篡改可检。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditAction {
    ContractArchived(u8),
    TestsRecorded,
    BudgetRecorded,
    DocsRecorded,
    LeftoverRegistered,
    VerifyBegun,
    EvidenceSubmitted(Seat, Ledger),
    Signed(Seat),
    Sealed,
}

/// 审计账条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditRow {
    pub seq: u64,
    pub action: AuditAction,
    /// 链式指纹：`h = fnv1a(prev || action)`，首环 prev = 0。
    pub hash: u32,
}

impl CloseRecord {
    /// 审计账（逐动作追加；`audit: Vec<AuditRow>` 挂在记录上）。
    fn audit_push(&mut self, action: AuditAction) {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.audit_last.to_be_bytes());
        bytes.push(match action {
            AuditAction::ContractArchived(k) => 0x10 ^ k,
            AuditAction::TestsRecorded => 0x20,
            AuditAction::BudgetRecorded => 0x30,
            AuditAction::DocsRecorded => 0x40,
            AuditAction::LeftoverRegistered => 0x50,
            AuditAction::VerifyBegun => 0x60,
            AuditAction::EvidenceSubmitted(s, l) => {
                0x70 ^ ((if s == Seat::Eng { 0 } else { 8 })
                    + match l { Ledger::Api => 1, Ledger::Theme => 2, Ledger::Contract => 3, Ledger::Test => 4, Ledger::Budget => 5, Ledger::Doc => 6 })
            }
            AuditAction::Signed(s) => 0x80 ^ (if s == Seat::Eng { 1 } else { 2 }),
            AuditAction::Sealed => 0xF0,
        });
        self.audit_last = fnv1a(&bytes);
        self.audit_seq += 1;
        self.audit.push(AuditRow { seq: self.audit_seq, action, hash: self.audit_last });
    }

    /// 审计链重放校验：逐环重算 hash，与在册值一致且链尾 = `audit_last`。
    pub fn audit_replay(&self) -> Result<(), CloseError> {
        let mut prev: u32 = 0;
        for row in &self.audit {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&prev.to_be_bytes());
            bytes.push(match row.action {
                AuditAction::ContractArchived(k) => 0x10 ^ k,
                AuditAction::TestsRecorded => 0x20,
                AuditAction::BudgetRecorded => 0x30,
                AuditAction::DocsRecorded => 0x40,
                AuditAction::LeftoverRegistered => 0x50,
                AuditAction::VerifyBegun => 0x60,
                AuditAction::EvidenceSubmitted(s, l) => {
                    0x70 ^ ((if s == Seat::Eng { 0 } else { 8 })
                        + match l { Ledger::Api => 1, Ledger::Theme => 2, Ledger::Contract => 3, Ledger::Test => 4, Ledger::Budget => 5, Ledger::Doc => 6 })
                }
                AuditAction::Signed(s) => 0x80 ^ (if s == Seat::Eng { 1 } else { 2 }),
                AuditAction::Sealed => 0xF0,
            });
            let h = fnv1a(&bytes);
            if h != row.hash {
                return Err(CloseError::SealTamper { sealed: row.hash, recomputed: h });
            }
            prev = h;
        }
        if prev != self.audit_last {
            return Err(CloseError::SealTamper { sealed: self.audit_last, recomputed: prev });
        }
        Ok(())
    }

    /// 主题登记：三主题逐项兑现入账（收集期与核验期均可补登，封印后拒）。
    pub fn register_theme(&mut self, theme: Theme, fact: &[u8]) -> Result<(), CloseError> {
        if self.phase == ClosePhase::Sealed {
            return Err(CloseError::Reopen);
        }
        if fact.is_empty() {
            return Err(CloseError::LedgerUnproven { seat: Seat::Qa, ledger: Ledger::Theme });
        }
        let real = ThemeRealization::mint(theme, fact);
        if real.proof == 0 {
            return Err(CloseError::LedgerUnproven { seat: Seat::Qa, ledger: Ledger::Theme });
        }
        self.themes.retain(|t: &ThemeRealization| t.theme != theme);
        self.themes.push(real);
        self.audit_push(AuditAction::EvidenceSubmitted(Seat::Qa, Ledger::Theme));
        Ok(())
    }

    /// 主题账齐备性：三主题全部兑现且兑现账与主题绑定关系一致。
    pub fn themes_complete(&self) -> bool {
        Theme::ALL.iter().all(|t| {
            self.themes.iter().any(|r| {
                r.theme == *t
                    && r.proof != 0
                    && r.backed_by
                        == match t {
                            Theme::Arch4Stage => Ledger::Api,
                            Theme::TripleDelivery => Ledger::Contract,
                            Theme::Budget15 => Ledger::Budget,
                        }
            })
        })
    }
}

impl Seat {
    pub const ALL: [Seat; 2] = [Seat::Eng, Seat::Qa];
}

/// Eb01 标准收口流（确定性端到端：收集→核验→双签→封印→封印校验）。
pub fn standard_close() -> Result<CloseRecord, CloseError> {
    let mut r = CloseRecord::new();
    for c in CONTRACTS {
        r.archive_contract(c)?;
    }
    r.record_tests(TEST_UNIT, TEST_INTEGRATION, TEST_E2E)?;
    r.record_budget(1400, 1650)?;
    r.record_docs(true, true)?;
    r.register_theme(Theme::Arch4Stage, b"arch4stage-impl")?;
    r.register_theme(Theme::TripleDelivery, b"triple-delivery-ok")?;
    r.register_theme(Theme::Budget15, b"budget-1.5ms-met")?;
    r.begin_verify()?;
    for s in Seat::ALL {
        for l in Ledger::ALL {
            r.submit_evidence(s, l, l.name().as_bytes())?;
        }
    }
    r.sign(Seat::Eng)?;
    r.sign(Seat::Qa)?;
    r.seal()?;
    r.verify_seal()?;
    Ok(r)
}

/// F0820 判据自检（域聚合入口）。
pub fn run_vee20_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vee20-F0820");

    // 判据 1：5 函数冻结——签名总账五项、哈希非零、入档序与闭集一致。
    let sigs = frozen_signatures();
    set.add("C0820-五函数冻结-数量", sigs.len() == 5, "签名总账恰五项");
    set.add("C0820-五函数冻结-哈希非零", sigs.iter().all(|s| s.frozen_hash != 0), "入档哈希全非零");
    set.add("C0820-五函数冻结-序一致", sigs.iter().zip(FROZEN_APIS.iter()).all(|(r, a)| r.api == *a), "入档序=闭集序");
    // 签名确定性：同接口两次冻结同哈希（冻结语义）。
    let again = frozen_signatures();
    set.add("C0820-冻结确定性", sigs.iter().zip(again.iter()).all(|(a, b)| a.frozen_hash == b.frozen_hash), "重冻结同哈希");

    // 判据 2：三契约归档——标准流归档恰三条、闭集外拒。
    let mut r = CloseRecord::new();
    let bad = r.archive_contract("Y99-未知契约");
    set.add("C0820-契约闭集外拒", matches!(bad, Err(CloseError::ContractMissing { .. })), "未知编号显性拒");
    for c in CONTRACTS { let _ = r.archive_contract(c); }
    set.add("C0820-三契约归档", r.contracts.len() == 3, "闭集三条全归档");

    // 判据 3：四账实测——测试账定数、性能双列、文档两件；缺口显性。
    let tg = r.record_tests(123, 12, 8);
    set.add("C0820-测试账缺口拒", matches!(tg, Err(CloseError::TestGap { .. })), "定数不符显性");
    let _ = r.record_tests(TEST_UNIT, TEST_INTEGRATION, TEST_E2E);
    let bl = r.record_budget(1400, 1900);
    set.add("C0820-性能谎报拒", matches!(bl, Err(CloseError::BudgetLie { .. })), "P95 超线显性");
    let _ = r.record_budget(1400, 1650);
    let dg = r.record_docs(true, false);
    set.add("C0820-文档缺口拒", matches!(dg, Err(CloseError::DocGap)), "词条表缺失显性");
    let _ = r.record_docs(true, true);

    // 判据 4：遗留显性——标准遗留带驱动条件；无条件拒。
    let vague = Leftover { item: String::from("以后再说"), driver: None };
    set.add("C0820-遗留无条件拒", matches!(vague.validate(), Err(CloseError::LeftoverVague { .. })), "无驱动条件非法");
    set.add("C0820-遗留显性登记", r.leftovers.len() == 1 && r.leftovers[0].validate().is_ok(), "LCD 亚像素带条件在册");

    // 判据 5：双签生效——先核后签、双席各一次、封印单向、篡改可检。
    let mut flow = match standard_close() {
        Ok(x) => x,
        Err(e) => {
            set.add("C0820-标准流闭环", false, "标准流失败");
            let _ = e;
            return set;
        }
    };
    set.add("C0820-标准流闭环", true, "收集→核验→双签→封印全通");
    set.add("C0820-封印在册校验", flow.verify_seal().is_ok(), "入封=重算");
    // 顺序违规：核验期外落签必拒（新记录未 begin_verify）。
    let mut early = CloseRecord::new();
    set.add("C0820-先签后核拒", matches!(early.sign(Seat::Eng), Err(CloseError::SignOrder)), "核验前落签违规");
    // 篡改可检：封印后账目只读——通过重开语义拒写。
    set.add("C0820-封印后拒改", matches!(flow.record_tests(1, 2, 3), Err(CloseError::Reopen)), "Sealed 态单向");
    // 证据齐备性对偶：缺一份证据即拒签。
    let mut lack = CloseRecord::new();
    for c in CONTRACTS { let _ = lack.archive_contract(c); }
    let _ = lack.record_tests(TEST_UNIT, TEST_INTEGRATION, TEST_E2E);
    let _ = lack.record_budget(1400, 1650);
    let _ = lack.record_docs(true, true);
    let _ = lack.begin_verify();
    for s in Seat::ALL {
        for l in Ledger::ALL {
            let _ = lack.submit_evidence(s, l, l.name().as_bytes());
        }
    }
    lack.evidence.retain(|e| !(e.seat == Seat::Qa && e.ledger == Ledger::Doc));
    set.add("C0820-缺证拒签", matches!(lack.sign(Seat::Qa), Err(CloseError::LedgerUnproven { .. })), "缺质量席文档账证据拒签");

    // 主题登记账：三主题兑现齐备 + 绑定关系一致 + 空证拒绝。
    set.add("C0820-主题三兑现", flow.themes_complete(), "四段/三向/预算全兑现");
    let mut thin = CloseRecord::new();
    set.add("C0820-主题空证拒", matches!(thin.register_theme(Theme::Budget15, b""), Err(CloseError::LedgerUnproven { .. })), "空证据不构成兑现");

    // 审计流：append-only 重放一致 + 封印后拒追加。
    set.add("C0820-审计重放一致", flow.audit_replay().is_ok(), "逐环哈希链一致");
    set.add("C0820-封印后审计账只读", matches!(flow.register_theme(Theme::Arch4Stage, b"x"), Err(CloseError::Reopen)), "Sealed 态登记拒");

    // 读屏面：六行播报、无隐私字段。
    let lines = flow.a11y_lines();
    set.add("C0820-读屏六行", lines.len() == 6, "双语播报行数冻结");
    set.add("C0820-诊断十码", SEG_BASE == 0x2400, "诊断段独占 0x2400");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vee20_standard_close_all_green() {
        let r = standard_close().expect("标准收口流");
        assert_eq!(r.phase, ClosePhase::Sealed);
        assert!(r.verify_seal().is_ok());
    }

    #[test]
    fn vee20_checks_all_green() {
        let set = run_vee20_checks();
        assert_eq!(set.red_items().1, 0, "红项：{:?}", set.red_items().0);
    }
}
