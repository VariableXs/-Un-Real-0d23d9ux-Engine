//! CGPU-F1709 · 异构报告生成
//!
//! 判据锚点（施工书原文摘录）：
//! 报告：报告族（异构周报（协同效果/能效/异常——周报模板复用 F1559 模式——模板；
//! 回源（数字回源断言复用；导出（复用 F1561——导出复用；测试（模板/回源/导出
//! 三组）。判据：模板、回源、导出、三组、判据。
//!
//! 诊断码段 0x9401~0x9405（独占）：
//!   0x9401 模板失格（节缺失/节越三闭集/必填行空）
//!   0x9402 回源断言破（数字无源锚/回源重算失配）
//!   0x9403 导出失约（格式越闭集/未授权导出/导出与正文指纹失配）
//!   0x9404 段越界（判码落 0x9400..0x9500 区间外）
//!   0x9405 保留哨（本段扩位预留——不派发）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。


/// 模块版本。
pub const REPORT_GEN_VERSION: &str = "K28-reportgen-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0x9401~0x9405）
// ---------------------------------------------------------------------------

pub const XC_TEMPLATE_MALFORMED: u32 = 0x9401;
pub const XC_BACKING_BROKEN: u32 = 0x9402;
pub const XC_EXPORT_VIOLATION: u32 = 0x9403;
pub const XC_SEG_OOR: u32 = 0x9404;
pub const XC_SEG_RESERVED: u32 = 0x9405;

// ---------------------------------------------------------------------------
// 一、周报模板（F1559 模式复用）
// ---------------------------------------------------------------------------

/// 周报节三闭集（判据"协同效果/能效/异常"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RSection {
    /// 协同效果节。
    Synergy,
    /// 能效节。
    Efficacy,
    /// 异常节。
    Anomaly,
}

impl RSection {
    pub const ALL: [RSection; 3] = [RSection::Synergy, RSection::Efficacy, RSection::Anomaly];
    pub const fn name(self) -> &'static str {
        match self {
            RSection::Synergy => "synergy",
            RSection::Efficacy => "efficacy",
            RSection::Anomaly => "anomaly",
        }
    }
}

/// 模板行：节 + 数据锚（注册表指标名）+ 必填标志。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TplRow {
    pub section: RSection,
    /// 数据锚（回源目标——必须能回源到注册表口径）。
    pub anchor: &'static str,
    pub required: bool,
}

/// 周报模板（F1559 复用：三节七行固定骨架）。
pub struct WeeklyTpl {
    rows: [TplRow; 7],
}

impl WeeklyTpl {
    pub const fn build() -> Self {
        WeeklyTpl {
            rows: [
                TplRow { section: RSection::Synergy, anchor: "speedup_bp", required: true },
                TplRow { section: RSection::Synergy, anchor: "handoff_overhead_us", required: true },
                TplRow { section: RSection::Synergy, anchor: "migration_count", required: false },
                TplRow { section: RSection::Efficacy, anchor: "fps_per_watt_bp", required: true },
                TplRow { section: RSection::Efficacy, anchor: "power_milliwatt", required: false },
                TplRow { section: RSection::Anomaly, anchor: "fallback_count", required: true },
                TplRow { section: RSection::Anomaly, anchor: "frame_skip_count", required: false },
            ],
        }
    }

    /// 模板闭合：三节全覆盖 + 必填行每节至少一条 + 锚非空（0x9401）。
    pub fn intact(&self) -> Result<(), u32> {
        for sec in RSection::ALL {
            let mut has = false;
            let mut req_has = false;
            for r in &self.rows {
                if r.section == sec {
                    has = true;
                    if r.required {
                        req_has = true;
                    }
                    if r.anchor.is_empty() {
                        return Err(XC_TEMPLATE_MALFORMED);
                    }
                }
            }
            if !has || !req_has {
                return Err(XC_TEMPLATE_MALFORMED);
            }
        }
        Ok(())
    }

    pub fn rows(&self) -> &[TplRow] {
        &self.rows
    }
}

// ---------------------------------------------------------------------------
// 二、数字回源断言（复用）
// ---------------------------------------------------------------------------

/// 回源源（宿主口径视图——按锚名提供当前值）。
pub trait BackingSource {
    fn value_of(&self, anchor: &str) -> Option<u64>;
}

/// 宿主样例源。
pub struct HostBacking {
    rows: [(&'static str, u64); 7],
}

impl HostBacking {
    pub fn build() -> Self {
        HostBacking {
            rows: [
                ("speedup_bp", 13_800),
                ("handoff_overhead_us", 42),
                ("migration_count", 7),
                ("fps_per_watt_bp", 5_000_000),
                ("power_milliwatt", 30_000),
                ("fallback_count", 2),
                ("frame_skip_count", 0),
            ],
        }
    }
}

impl BackingSource for HostBacking {
    fn value_of(&self, anchor: &str) -> Option<u64> {
        let mut found = None;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].0 == anchor {
                found = Some(self.rows[i].1);
            }
            i += 1;
        }
        found
    }
}

/// 回源断言：报告里的每个数字必须 == 源锚现值（0x9402）。
pub fn backing_check(tpl: &WeeklyTpl, src: &dyn BackingSource, reported: &[u64]) -> Result<(), u32> {
    let rows = tpl.rows();
    if reported.len() != rows.len() {
        return Err(XC_BACKING_BROKEN);
    }
    let mut i = 0;
    while i < rows.len() {
        match src.value_of(rows[i].anchor) {
            Some(v) if v == reported[i] => {}
            _ => return Err(XC_BACKING_BROKEN),
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、导出（F1561 复用）
// ---------------------------------------------------------------------------

/// 导出格式三闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportFmt {
    /// 纯文本（控制台/串口）。
    Text,
    /// Markdown。
    Markdown,
    /// 结构化行（键值对——机器回读）。
    Structured,
}

impl ExportFmt {
    pub const ALL: [ExportFmt; 3] =
        [ExportFmt::Text, ExportFmt::Markdown, ExportFmt::Structured];
    pub const fn name(self) -> &'static str {
        match self {
            ExportFmt::Text => "text",
            ExportFmt::Markdown => "markdown",
            ExportFmt::Structured => "structured",
        }
    }
}

/// FNV-1a 64。
pub const fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf_29ce_4842_22325;
    let mut i = 0;
    while i < bytes.len() {
        h = (h ^ bytes[i] as u64).wrapping_mul(0x100_0000_01b3);
        i += 1;
    }
    h
}

/// 导出闸：格式合法 + 授权标志 + 正文指纹（导出物与正文一致——0x9403）。
pub struct ExportGate {
    /// 授权（显性开才许导出——隐私默认关复用）。
    pub authorized: bool,
    /// 导出次数（审计）。
    pub exports: u32,
}

impl ExportGate {
    pub const fn build() -> Self {
        ExportGate { authorized: false, exports: 0 }
    }

    /// 导出：格式闭集校验（枚举保证）+ 授权 + 指纹对拍。
    pub fn emit(&mut self, fmt: ExportFmt, body: &[u8], body_fp: u64) -> Result<u64, u32> {
        let _ = fmt.name(); // 闭集枚举自证。
        if !self.authorized {
            return Err(XC_EXPORT_VIOLATION);
        }
        let fp = fnv64(body);
        if fp != body_fp {
            return Err(XC_EXPORT_VIOLATION);
        }
        self.exports += 1;
        Ok(fp)
    }
}

// ---------------------------------------------------------------------------
// 自检（三组：模板/回源/导出）
// ---------------------------------------------------------------------------

/// 报告引擎。
pub struct ReportEngine {
    pub tpl: WeeklyTpl,
    pub gate: ExportGate,
}

impl ReportEngine {
    pub fn new() -> Self {
        ReportEngine { tpl: WeeklyTpl::build(), gate: ExportGate::build() }
    }

    /// 组一：模板——三节全盖 + 必填齐 + 锚非空。
    pub fn selfcheck_template(&self) -> Result<(), u32> {
        self.tpl.intact()?;
        if self.tpl.rows().len() != 7 {
            return Err(XC_TEMPLATE_MALFORMED);
        }
        // 每节行数核对：协同 3 / 能效 2 / 异常 2。
        let mut cnt = [0u8; 3];
        for r in self.tpl.rows() {
            cnt[r.section as usize] += 1;
        }
        if cnt != [3, 2, 2] {
            return Err(XC_TEMPLATE_MALFORMED);
        }
        Ok(())
    }

    /// 组二：回源——全对过 + 失配拦 + 长度失配拦。
    pub fn selfcheck_backing(&self) -> Result<(), u32> {
        let src = HostBacking::build();
        let good = [13_800, 42, 7, 5_000_000, 30_000, 2, 0];
        backing_check(&self.tpl, &src, &good)?;
        // 失配反例。
        let mut bad = good;
        bad[0] = 13_801;
        if backing_check(&self.tpl, &src, &bad).is_ok() {
            return Err(XC_BACKING_BROKEN);
        }
        // 长度失配反例。
        let short = [13_800, 42];
        if backing_check(&self.tpl, &src, &short).is_ok() {
            return Err(XC_BACKING_BROKEN);
        }
        Ok(())
    }

    /// 组三：导出——未授权拒 + 授权放行 + 指纹失配拦 + 审计累计。
    pub fn selfcheck_export(&mut self) -> Result<(), u32> {
        let body = b"Weekly synergy 13800bp";
        let fp = fnv64(body);
        // 未授权拒。
        if self.gate.emit(ExportFmt::Markdown, body, fp).is_ok() {
            return Err(XC_EXPORT_VIOLATION);
        }
        self.gate.authorized = true;
        let out = self.gate.emit(ExportFmt::Markdown, body, fp)?;
        if out != fp {
            return Err(XC_EXPORT_VIOLATION);
        }
        // 指纹失配拦。
        if self.gate.emit(ExportFmt::Text, body, fp + 1).is_ok() {
            return Err(XC_EXPORT_VIOLATION);
        }
        if self.gate.exports != 1 {
            return Err(XC_EXPORT_VIOLATION);
        }
        // 格式闭集自证（三格式名非空）。
        for f in ExportFmt::ALL {
            if f.name().is_empty() {
                return Err(XC_EXPORT_VIOLATION);
            }
        }
        Ok(())
    }

    /// 全量自检入口（三组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        Self::segment_guard()?;
        self.selfcheck_template()?;
        self.selfcheck_backing()?;
        self.selfcheck_export()?;
        Ok(())
    }

    /// 段对账：五码全落 0x9400..0x9500 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_TEMPLATE_MALFORMED,
            XC_BACKING_BROKEN,
            XC_EXPORT_VIOLATION,
            XC_SEG_OOR,
            XC_SEG_RESERVED,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x9400 || codes[i] >= 0x9500 {
                return Err(XC_TEMPLATE_MALFORMED);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_TEMPLATE_MALFORMED);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
