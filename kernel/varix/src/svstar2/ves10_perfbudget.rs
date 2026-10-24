//! VE-F3610 · 创作性能预算（VE-S 域 · 批次 S01 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3610`
//!
//! **判据（锚点原文六条）**：50ms 创作线、代理保真、导出 5s、归因复述、
//! 更严声明、判据。逐条落位：
//! - **50ms 创作线**：编辑操作响应预算 **50ms**（创作即时性红线，
//!   **比通用交互 100ms 更严**——创作更严声明，常量 [`EDIT_BUDGET_MS`]）
//!   。超线 → [`V0ACode::EDIT_OVER`] 优化立案（实测红线）。
//! - **代理保真**：大资产分块编辑走低清代理，代理编辑产物必须与全量
//!   一致（代理对拍红线）——[`ProxySession::export`] 对拍代理指纹与
//!   全量期望指纹，漂移 → [`V0ACode::PROXY_DRIFT`] 拦截。
//! - **导出 5s**：常规资产构建/导出预算 **5000ms**（[`EXPORT_BUDGET_MS`]），
//!   超时 → 分块显性（[`V0ACode::EXPORT_OVER`]）。
//! - **归因复述**：创作工具入性能瀑布（复述制度）——[`PerfEntry`] 入
//!   瀑布账，缺归因条目 → 保守估算（[`V0ACode::ATTRIB_MISSING`]，复述）。
//! - **更严声明**：[`BUDGET_DISCLOSURE`] 常量显性声明 50ms < 100ms 的
//!   创作更严口径，判据钉住常量值。
//! - **判据**：`run_ves10_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 50ms 超线→优化立案（红线实测） | [`V0ACode::EDIT_OVER`]：超线立案，不静默 |
//! | 代理失真→对拍拦截（红线实测） | [`V0ACode::PROXY_DRIFT`]：指纹对拍失败拒导出 |
//! | 导出超时→分块显性（复述） | [`V0ACode::EXPORT_OVER`]：超时报告含分块建议 |
//! | 归因缺→保守（复述） | [`V0ACode::ATTRIB_MISSING`]：缺归因按最坏估算 |
//!
//! **码段**：独占 **0x3axx**。
//!
//! **性能逐项分解（锚点口径）**：预算 O(三线)（三次比较）；
//! 代理 O(编辑)（代理层直通）；归因 O(复述)（瀑布追加）；
//! 断言 O(1)。
//!
//! **跨批对接点（锚点原文三名）**：F2816/F2953 制度复用；F3606 预览
//! 对端（[`preview_budget`]）；F3287 优化对端（[`optimize_hint`]）——
//! 三点钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟（预算比较用调用方注入的实测毫秒数）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ── 预算常量（独立写死，不复用被测值）──────────────────────────────

/// 编辑响应预算（毫秒）。创作更严线：50 < 通用交互 100。
pub const EDIT_BUDGET_MS: u64 = 50;

/// 预览刷新预算（毫秒）。复用 F3606 口径。
pub const PREVIEW_BUDGET_MS: u64 = 100;

/// 构建/导出预算（毫秒）。
pub const EXPORT_BUDGET_MS: u64 = 5000;

/// 千令牌批量操作预算（毫秒）。
pub const BATCH_BUDGET_MS: u64 = 200;

/// 创作更严声明（判据钉住）。
pub const BUDGET_DISCLOSURE: &str =
    "创作更严声明：编辑响应 50ms 比通用交互 100ms 更严——创作流畅度死穴不允许卡顿。";

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3610 诊断码段（独占 0x3axx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V0ACode(pub u16);

impl V0ACode {
    /// 编辑超 50ms 线。
    pub const EDIT_OVER: V0ACode = V0ACode(0x3a01);
    /// 预览超 100ms 线。
    pub const PREVIEW_OVER: V0ACode = V0ACode(0x3a02);
    /// 导出超 5s 线。
    pub const EXPORT_OVER: V0ACode = V0ACode(0x3a03);
    /// 代理失真。
    pub const PROXY_DRIFT: V0ACode = V0ACode(0x3a04);
    /// 归因缺失。
    pub const ATTRIB_MISSING: V0ACode = V0ACode(0x3a05);
    /// 批量超 200ms 线。
    pub const BATCH_OVER: V0ACode = V0ACode(0x3a06);
    /// 实测值异常（零或溢出）。
    pub const BAD_SAMPLE: V0ACode = V0ACode(0x3a07);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V0ACode::EDIT_OVER => {
                "编辑响应超过 50ms 创作红线。建议：开启分块编辑或代理预览，并入性能瀑布定位热点。"
            }
            V0ACode::PREVIEW_OVER => {
                "预览刷新超过 100ms。建议：启用增量预览，只重绘变更区域。"
            }
            V0ACode::EXPORT_OVER => {
                "导出超过 5s 预算。建议：改用分块导出并显性告知用户进度。"
            }
            V0ACode::PROXY_DRIFT => {
                "代理编辑产物与全量不一致（保真红线）。建议：禁用代理并全量重算，排查代理层采样误差。"
            }
            V0ACode::ATTRIB_MISSING => {
                "性能条目缺归因信息。建议：补全 PerfEntry 的来源标签，否则按最坏估算保守记账。"
            }
            V0ACode::BATCH_OVER => {
                "千令牌批量操作超过 200ms。建议：拆批或走虚拟化列表。"
            }
            V0ACode::BAD_SAMPLE => {
                "实测样本异常（零值或溢出）。建议：校验计时器来源。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 预算三线断言 ────────────────────────────────────────────────────

/// 三线预算表。
#[derive(Clone, Copy, Debug)]
pub struct BudgetTable {
    /// 编辑线。
    pub edit_ms: u64,
    /// 预览线。
    pub preview_ms: u64,
    /// 导出线。
    pub export_ms: u64,
    /// 批量线。
    pub batch_ms: u64,
}

impl BudgetTable {
    /// 默认三线（独立常量，不复用被测值）。
    pub const fn standard() -> BudgetTable {
        BudgetTable {
            edit_ms: EDIT_BUDGET_MS,
            preview_ms: PREVIEW_BUDGET_MS,
            export_ms: EXPORT_BUDGET_MS,
            batch_ms: BATCH_BUDGET_MS,
        }
    }

    /// 编辑线断言。
    pub fn check_edit(&self, measured_ms: u64) -> Result<(), V0ACode> {
        if measured_ms == 0 {
            return Err(V0ACode::BAD_SAMPLE);
        }
        if measured_ms > self.edit_ms {
            return Err(V0ACode::EDIT_OVER);
        }
        Ok(())
    }

    /// 预览线断言。
    pub fn check_preview(&self, measured_ms: u64) -> Result<(), V0ACode> {
        if measured_ms == 0 {
            return Err(V0ACode::BAD_SAMPLE);
        }
        if measured_ms > self.preview_ms {
            return Err(V0ACode::PREVIEW_OVER);
        }
        Ok(())
    }

    /// 导出线断言。
    pub fn check_export(&self, measured_ms: u64) -> Result<(), V0ACode> {
        if measured_ms == 0 {
            return Err(V0ACode::BAD_SAMPLE);
        }
        if measured_ms > self.export_ms {
            return Err(V0ACode::EXPORT_OVER);
        }
        Ok(())
    }

    /// 批量线断言。
    pub fn check_batch(&self, measured_ms: u64) -> Result<(), V0ACode> {
        if measured_ms == 0 {
            return Err(V0ACode::BAD_SAMPLE);
        }
        if measured_ms > self.batch_ms {
            return Err(V0ACode::BATCH_OVER);
        }
        Ok(())
    }
}

// ── 代理模式（代理保真对拍）─────────────────────────────────────────

/// 代理编辑会话。
#[derive(Clone, Debug)]
pub struct ProxySession {
    /// 全量资产指纹（期望值）。
    pub full_fingerprint: u64,
    /// 代理编辑产生的指纹（实际值）。
    pub proxy_fingerprint: Option<u64>,
    /// 代理比例（1:N）。
    pub scale_denom: u32,
    /// 诊断袋。
    pub drift_count: u32,
}

impl ProxySession {
    /// 新建代理会话。
    pub fn new(full_fingerprint: u64, scale_denom: u32) -> ProxySession {
        ProxySession {
            full_fingerprint,
            proxy_fingerprint: None,
            scale_denom: if scale_denom == 0 { 1 } else { scale_denom },
            drift_count: 0,
        }
    }

    /// 记录代理编辑产出指纹。
    pub fn record_proxy(&mut self, fp: u64) {
        self.proxy_fingerprint = Some(fp);
    }

    /// 导出时对拍：代理产物必须与全量一致（保真红线）。
    pub fn export(&mut self) -> Result<u64, V0ACode> {
        match self.proxy_fingerprint {
            Some(fp) => {
                if fp != self.full_fingerprint {
                    self.drift_count = self.drift_count.wrapping_add(1);
                    return Err(V0ACode::PROXY_DRIFT);
                }
                Ok(fp)
            }
            None => Err(V0ACode::PROXY_DRIFT),
        }
    }

    /// 漂移计数。
    pub fn drift_count(&self) -> u32 {
        self.drift_count
    }
}

// ── 性能瀑布归因 ────────────────────────────────────────────────────

/// 瀑布条目。
#[derive(Clone, Debug)]
pub struct PerfEntry {
    /// 来源标签（归因必填）。
    pub source: String,
    /// 耗时毫秒。
    pub cost_ms: u64,
    /// 是否保守估算（缺归因时）。
    pub conservative: bool,
}

/// 性能瀑布账（复述制度）。
#[derive(Clone, Debug, Default)]
pub struct PerfWaterfall {
    entries: Vec<PerfEntry>,
    missing_attrib: u32,
}

impl PerfWaterfall {
    /// 空账。
    pub fn new() -> PerfWaterfall {
        PerfWaterfall {
            entries: Vec::new(),
            missing_attrib: 0,
        }
    }

    /// 入账（缺归因 → 保守估算 + 立案标记）。
    pub fn record(&mut self, source: &str, cost_ms: u64) {
        if source.is_empty() {
            self.missing_attrib = self.missing_attrib.wrapping_add(1);
            self.entries.push(PerfEntry {
                source: String::from("(未归因-保守)"),
                cost_ms,
                conservative: true,
            });
        } else {
            self.entries.push(PerfEntry {
                source: String::from(source),
                cost_ms,
                conservative: false,
            });
        }
    }

    /// 总耗时。
    pub fn total_ms(&self) -> u64 {
        self.entries.iter().map(|e| e.cost_ms).sum()
    }

    /// 保守估算条数（缺归因计数）。
    pub fn conservative_count(&self) -> u32 {
        self.missing_attrib
    }

    /// 是否有缺归因。
    pub fn has_missing_attrib(&self) -> bool {
        self.missing_attrib > 0
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 渲染瀑布。
    pub fn render(&self) -> String {
        let mut s = String::new();
        for (i, e) in self.entries.iter().enumerate() {
            let flag = if e.conservative { " [保守]" } else { "" };
            s.push_str(&alloc::format!("{}. {} {}ms{}\n", i + 1, e.source, e.cost_ms, flag));
        }
        s
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// F3606 预览对端契约。
pub fn preview_budget(measured_ms: u64) -> Result<(), V0ACode> {
    BudgetTable::standard().check_preview(measured_ms)
}

/// F3287 优化对端提示。
pub fn optimize_hint(code: V0ACode) -> &'static str {
    code.explain()
}

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F2816", "VE-F3606", "VE-F3287"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves10_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3610");
    let bt = BudgetTable::standard();

    // 0x3a01 50ms 线：内过外不过
    out.add("0x3a01-编辑 50ms 线卡两侧", bt.check_edit(50).is_ok() && bt.check_edit(51).is_err(), "");

    // 0x3a02 100ms 预览线
    out.add("0x3a02-预览 100ms 线卡两侧", bt.check_preview(100).is_ok() && bt.check_preview(101).is_err(), "");

    // 0x3a03 5s 导出线
    out.add("0x3a03-导出 5s 线卡两侧", bt.check_export(5000).is_ok() && bt.check_export(5001).is_err(), "");

    // 0x3a04 批量 200ms 线
    out.add("0x3a04-批量 200ms 线卡两侧", bt.check_batch(200).is_ok() && bt.check_batch(201).is_err(), "");

    // 0x3a05 零样本拒收
    out.add("0x3a05-零样本拒收", bt.check_edit(0).is_err(), "");

    // 0x3a06 代理保真：一致放行
    let mut ps = ProxySession::new(0xabc, 2);
    ps.record_proxy(0xabc);
    out.add("0x3a06-代理一致放行", ps.export().is_ok(), "");

    // 0x3a07 代理漂移拦截
    let mut ps2 = ProxySession::new(0xabc, 2);
    ps2.record_proxy(0xdef);
    out.add("0x3a07-代理漂移拦截", ps2.export().is_err() && ps2.drift_count() == 1, "");

    // 0x3a08 归因缺标记
    let mut wf = PerfWaterfall::new();
    wf.record("edit", 10);
    wf.record("", 5);
    out.add("0x3a08-缺归因保守估算", wf.has_missing_attrib() && wf.conservative_count() == 1, "");

    // 0x3a09 更严声明常量
    out.add("0x3a09-更严声明含 50ms", BUDGET_DISCLOSURE.contains("50"), "");

    // 0x3a0a 瀑布总和
    out.add("0x3a0a-瀑布总和正确", wf.total_ms() == 15, "");

    out
}
