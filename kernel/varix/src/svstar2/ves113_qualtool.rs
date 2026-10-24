//! VE-F3713 · 质量工具集成
//!
//! 扫描工具对外集成：CI / 编辑器内嵌 / 第三方 API。
//! 同源红线 P0：CI 扫描与编辑器扫描结果必须一致（同输入同输出）。
//! API 三段硬门；内嵌提示非阻塞。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xA1xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xA100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CA1Code(pub u16);

impl CA1Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const SOURCE_DRIFT: Self = Self(SEG_BASE + 1);
    pub const API_MISSING: Self = Self(SEG_BASE + 2);
    pub const HINT_BLOCKING: Self = Self(SEG_BASE + 3);
    pub const VERSION_DRIFT: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::SOURCE_DRIFT => "同源破检出（P0）：CI 与编辑器扫描结果不一致",
            Self::API_MISSING => "API 缺段，不可发布",
            Self::HINT_BLOCKING => "编辑器提示阻塞了编辑流",
            Self::VERSION_DRIFT => "版本漂移，需对齐",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 扫描结果摘要
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanResult {
    pub score: u16,
    pub issue_count: u16,
    pub fingerprint: u32,
}

impl ScanResult {
    pub fn new(score: u16, issue_count: u16) -> Self {
        let fingerprint = (score as u32) << 16 | issue_count as u32;
        Self { score, issue_count, fingerprint }
    }
}

/// 集成端
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    Ci,
    Editor,
    ThirdParty,
}

/// API 三段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiSegments {
    pub auth: bool,
    pub scan: bool,
    pub report: bool,
}

impl ApiSegments {
    pub fn complete(&self) -> bool {
        self.auth && self.scan && self.report
    }
}

/// 集成配置
#[derive(Debug, Clone)]
pub struct ToolIntegration {
    pub version: u16,
    pub api: ApiSegments,
    pub hint_nonblocking: bool,
}

impl ToolIntegration {
    pub fn standard() -> Self {
        Self {
            version: 1,
            api: ApiSegments { auth: true, scan: true, report: true },
            hint_nonblocking: true,
        }
    }

    /// API 三段硬门
    pub fn assert_api_complete(&self) -> Result<(), CA1Code> {
        if self.api.complete() {
            Ok(())
        } else {
            Err(CA1Code::API_MISSING)
        }
    }
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct QualTool {
    config: ToolIntegration,
    ci_results: Vec<ScanResult>,
    editor_results: Vec<ScanResult>,
}

impl QualTool {
    pub fn new(config: ToolIntegration) -> Self {
        Self { config, ci_results: Vec::new(), editor_results: Vec::new() }
    }

    /// CI 扫描（复用 F3272 模式）
    pub fn ci_scan(&mut self, result: ScanResult) -> Result<(), CA1Code> {
        self.config.assert_api_complete()?;
        self.ci_results.push(result);
        Ok(())
    }

    /// 编辑器内嵌扫描（非阻塞）
    pub fn editor_scan(&mut self, result: ScanResult) -> Result<(), CA1Code> {
        if !self.config.hint_nonblocking {
            return Err(CA1Code::HINT_BLOCKING);
        }
        self.editor_results.push(result);
        Ok(())
    }

    /// 同源断言 P0：双端对拍，同输入同输出
    pub fn assert_same_source(&self) -> Result<(), CA1Code> {
        if self.ci_results.len() != self.editor_results.len() {
            return Err(CA1Code::SOURCE_DRIFT);
        }
        for (c, e) in self.ci_results.iter().zip(self.editor_results.iter()) {
            if c.fingerprint != e.fingerprint {
                return Err(CA1Code::SOURCE_DRIFT);
            }
        }
        Ok(())
    }

    /// 版本对齐
    pub fn assert_version(&self, expected: u16) -> Result<(), CA1Code> {
        if self.config.version == expected {
            Ok(())
        } else {
            Err(CA1Code::VERSION_DRIFT)
        }
    }

    pub fn third_party_export(&self) -> Result<(u16, bool, bool), CA1Code> {
        self.config.assert_api_complete()?;
        Ok((self.config.version, self.config.api.auth, self.config.api.report))
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves113_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves113_qualtool");

    // 01 同源 P0：结果不一致
    {
        let mut t = QualTool::new(ToolIntegration::standard());
        t.ci_scan(ScanResult::new(90, 2)).unwrap();
        t.editor_scan(ScanResult::new(85, 3)).unwrap();
        chk(&mut set, "C3713-01", t.assert_same_source() == Err(CA1Code::SOURCE_DRIFT), "err");
    }
    // 02 同源一致
    {
        let mut t = QualTool::new(ToolIntegration::standard());
        let r = ScanResult::new(90, 2);
        t.ci_scan(r.clone()).unwrap();
        t.editor_scan(r).unwrap();
        chk(&mut set, "C3713-02", t.assert_same_source().is_ok(), "err");
    }
    // 03 API 缺段
    {
        let cfg = ToolIntegration { version: 1, api: ApiSegments { auth: true, scan: false, report: true }, hint_nonblocking: true };
        let mut t = QualTool::new(cfg);
        chk(&mut set, "C3713-03", t.ci_scan(ScanResult::new(90, 0)) == Err(CA1Code::API_MISSING), "err");
    }
    // 04 提示阻塞
    {
        let cfg = ToolIntegration { version: 1, api: ApiSegments { auth: true, scan: true, report: true }, hint_nonblocking: false };
        let mut t = QualTool::new(cfg);
        chk(&mut set, "C3713-04", t.editor_scan(ScanResult::new(90, 0)) == Err(CA1Code::HINT_BLOCKING), "err");
    }
    // 05 版本漂移
    {
        let t = QualTool::new(ToolIntegration::standard());
        chk(&mut set, "C3713-05", t.assert_version(2) == Err(CA1Code::VERSION_DRIFT), "err");
    }
    // 06 版本一致
    {
        let t = QualTool::new(ToolIntegration::standard());
        chk(&mut set, "C3713-06", t.assert_version(1).is_ok(), "err");
    }
    // 07 数量不等也判漂移
    {
        let mut t = QualTool::new(ToolIntegration::standard());
        t.ci_scan(ScanResult::new(90, 2)).unwrap();
        chk(&mut set, "C3713-07", t.assert_same_source().is_err(), "err");
    }
    // 08 第三方导出
    {
        let t = QualTool::new(ToolIntegration::standard());
        chk(&mut set, "C3713-08", t.third_party_export().is_ok(), "err");
    }
    // 09 指纹确定性
    {
        let a = ScanResult::new(80, 5);
        let b = ScanResult::new(80, 5);
        chk(&mut set, "C3713-09", a.fingerprint == b.fingerprint, "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3713-10", !CA1Code::SOURCE_DRIFT.explain().is_empty(), "err");
    }

    set
}
