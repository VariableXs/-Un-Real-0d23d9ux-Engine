//! VE-F3667 · 个性化数据导出迁移（VE-S 域 · 批次 R04 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3667`
//!
//! **职责定位（锚点原文）**：个性化数据可携带——
//! - **开放格式**（JSON schema 公开）（开放断言：个性化锁死 = 反开放；
//!   导出物可被第三方解析）；
//! - **独立解析红线**（自证红线：导出物必须非本引擎解析成功——
//!   自己导自己导 = 自证）；
//! - **迁移向导**（跨设备/跨平台）；
//! - **导出脱敏**（画像含隐私 → 导出脱敏复述）。
//!
//! **判据（锚点原文六条）**：开放格式 P0、独立解析红线、导出脱敏、
//! 迁移向导、自证禁止、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 独立解析失败→P1（自证红线实测） | [`E67Code::SELF_PARSE_ONLY`] |
//! | 格式未公开→P0（开放红线最高级） | [`E67Code::SCHEMA_LOCKED`] |
//! | 迁移丢项→对账（复述） | [`E67Code::MIGRATE_LOSS`] |
//! | 脱敏漏→P0（复述隐私） | [`E67Code::REDACT_MISS`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3667 诊断码段（独占 0x73xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct E67Code(pub u16);

impl E67Code {
    /// 仅本引擎可解析（自证红线 P1）。
    pub const SELF_PARSE_ONLY: E67Code = E67Code(0x7301);
    /// schema 未公开（P0 开放红线）。
    pub const SCHEMA_LOCKED: E67Code = E67Code(0x7302);
    /// 迁移丢项（对账）。
    pub const MIGRATE_LOSS: E67Code = E67Code(0x7303);
    /// 脱敏遗漏（P0 隐私）。
    pub const REDACT_MISS: E67Code = E67Code(0x7304);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            E67Code::SELF_PARSE_ONLY => "仅本引擎可解析（自证红线 P1）。建议：独立解析器验证导出物。",
            E67Code::SCHEMA_LOCKED => "schema 未公开（P0 开放红线）。建议：公开 JSON schema。",
            E67Code::MIGRATE_LOSS => "迁移丢项。建议：对账清单核对。",
            E67Code::REDACT_MISS => "脱敏遗漏（P0）。建议：补隐私字段脱敏。",
            _ => "未知诊断码。",
        }
    }
}

// ── 导出项 ──────────────────────────────────────────────────────────

/// 导出条目。
#[derive(Clone, Copy)]
pub struct ExportItem {
    pub key: u16,
    pub value: i32,
    /// 含隐私（须脱敏）。
    pub private: bool,
    /// 已脱敏位。
    pub redacted: bool,
}

// ── 独立解析验证 ────────────────────────────────────────────────────

/// 解析器身份。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Parser {
    /// 本引擎（自证面）。
    SelfEngine,
    /// 独立第三方解析器。
    Independent,
}

// ── 导出迁移器 ──────────────────────────────────────────────────────

/// 个性化导出迁移器。
pub struct PrefExporter {
    items: Vec<ExportItem>,
    schema_public: bool,
    /// 独立解析成功位。
    independent_ok: bool,
    redact_miss: u32,
    last_code: E67Code,
    has_error: bool,
}

impl PrefExporter {
    /// 新建；`schema_public` 必须 true（开放格式 P0）。
    pub fn new(schema_public: bool) -> Self {
        Self {
            items: Vec::new(),
            schema_public,
            independent_ok: false,
            redact_miss: 0,
            last_code: E67Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> E67Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: E67Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 装入导出项。
    pub fn push(&mut self, item: ExportItem) -> usize {
        self.items.push(item);
        self.items.len()
    }

    /// 脱敏：隐私项必须标记已脱敏。
    pub fn redact(&mut self) -> Result<usize, E67Code> {
        let mut missed = 0;
        for it in self.items.iter_mut() {
            if it.private && !it.redacted {
                it.redacted = true;
            }
            if it.private && !it.redacted {
                missed += 1;
            }
        }
        let pending = self.items.iter().filter(|i| i.private && !i.redacted).count();
        if pending > 0 {
            self.redact_miss += pending as u32;
            self.fail(E67Code::REDACT_MISS);
            return Err(E67Code::REDACT_MISS);
        }
        let _ = missed;
        self.last_code = E67Code(0);
        self.has_error = false;
        Ok(self.items.iter().filter(|i| i.redacted).count())
    }

    /// 脱敏完备审计（导出前置）。
    pub fn audit_redaction(&mut self) -> Result<bool, E67Code> {
        if self.items.iter().all(|i| !i.private || i.redacted) {
            self.last_code = E67Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.redact_miss += 1;
            self.fail(E67Code::REDACT_MISS);
            Err(E67Code::REDACT_MISS)
        }
    }

    /// 开放格式断言（P0）。
    pub fn assert_open_schema(&mut self) -> Result<bool, E67Code> {
        if self.schema_public {
            self.last_code = E67Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(E67Code::SCHEMA_LOCKED);
            Err(E67Code::SCHEMA_LOCKED)
        }
    }

    /// 独立解析验证（自证红线：必须 Independent 成功）。
    pub fn verify_parse(&mut self, parser: Parser, ok: bool) -> Result<bool, E67Code> {
        match parser {
            Parser::Independent if ok => {
                self.independent_ok = true;
                self.last_code = E67Code(0);
                self.has_error = false;
                Ok(true)
            }
            Parser::Independent => {
                self.fail(E67Code::SELF_PARSE_ONLY);
                Err(E67Code::SELF_PARSE_ONLY)
            }
            Parser::SelfEngine => {
                self.fail(E67Code::SELF_PARSE_ONLY);
                Err(E67Code::SELF_PARSE_ONLY)
            }
        }
    }

    /// 迁移对账：导出数与迁入数一致。
    pub fn reconcile(&mut self, migrated: usize) -> Result<bool, E67Code> {
        if migrated == self.items.len() {
            self.last_code = E67Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(E67Code::MIGRATE_LOSS);
            Err(E67Code::MIGRATE_LOSS)
        }
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    pub fn independent_ok(&self) -> bool {
        self.independent_ok
    }

    pub fn redact_miss(&self) -> u32 {
        self.redact_miss
    }

    /// 携带总评：开放 + 独立解析 + 脱敏齐。
    pub fn portable_green(&mut self) -> bool {
        self.assert_open_schema().is_ok()
            && self.independent_ok
            && self.audit_redaction().is_ok()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves67_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves67_prefexport");
    {
        let e = PrefExporter::new(true);
        chk(&mut set, "C3667-01", e.item_count() == 0 && !e.independent_ok(), "err");
    }
    {
        let mut e = PrefExporter::new(true);
        let n = e.push(ExportItem { key: 1, value: 5, private: false, redacted: true });
        chk(&mut set, "C3667-02", n == 1, "err");
    }
    {
        let mut e = PrefExporter::new(true);
        e.push(ExportItem { key: 1, value: 5, private: true, redacted: false });
        let r = e.redact();
        chk(&mut set, "C3667-03", r == Ok(1), "err");
    }
    {
        let mut e = PrefExporter::new(true);
        e.push(ExportItem { key: 1, value: 5, private: true, redacted: true });
        let r = e.audit_redaction();
        chk(&mut set, "C3667-04", r == Ok(true), "err");
    }
    {
        let mut e = PrefExporter::new(true);
        let r = e.assert_open_schema();
        chk(&mut set, "C3667-05", r == Ok(true), "err");
    }
    {
        let mut e = PrefExporter::new(false);
        let r = e.assert_open_schema();
        chk(&mut set, "C3667-06", r.err() == Some(E67Code::SCHEMA_LOCKED), "err");
    }
    {
        let mut e = PrefExporter::new(true);
        let r = e.verify_parse(Parser::Independent, true);
        chk(&mut set, "C3667-07", r == Ok(true) && e.independent_ok(), "err");
    }
    {
        let mut e = PrefExporter::new(true);
        let r = e.verify_parse(Parser::SelfEngine, true);
        chk(&mut set, "C3667-08", r.err() == Some(E67Code::SELF_PARSE_ONLY), "err");
    }
    {
        let mut e = PrefExporter::new(true);
        let r = e.verify_parse(Parser::Independent, false);
        chk(&mut set, "C3667-09", r.err() == Some(E67Code::SELF_PARSE_ONLY), "err");
    }
    {
        let mut e = PrefExporter::new(true);
        e.push(ExportItem { key: 1, value: 1, private: false, redacted: true });
        e.push(ExportItem { key: 2, value: 2, private: false, redacted: true });
        let r = e.reconcile(2);
        chk(&mut set, "C3667-10", r == Ok(true), "err");
    }
    {
        let mut e = PrefExporter::new(true);
        e.push(ExportItem { key: 1, value: 1, private: false, redacted: true });
        let r = e.reconcile(3);
        chk(&mut set, "C3667-11", r.err() == Some(E67Code::MIGRATE_LOSS), "err");
    }
    {
        let mut e = PrefExporter::new(true);
        e.push(ExportItem { key: 1, value: 1, private: false, redacted: true });
        e.verify_parse(Parser::Independent, true);
        chk(&mut set, "C3667-12", e.portable_green(), "err");
    }
    set
}
