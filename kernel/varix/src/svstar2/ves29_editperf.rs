//! VE-F3629 · 编辑器性能（VE-S 域 · 批次 R02 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3629`
//!
//! **判据（锚点原文六条）**：50ms 复述、批量 200ms、链上限、大主题不卡、
//! 单源复用、判据。逐条落位：
//! - **50ms 复述**：编辑响应 ≤50ms（复述创作红线）。
//! - **批量 200ms**：千令牌批量 ≤200ms（批量预算）。
//! - **链上限**：undo 链内存上限（链内存红线：无限撤销 = 内存爆）——
//!   [`UndoBudget::push`] 超上限 → [`V1DCode::CHAIN_OVERFLOW`]。
//! - **大主题不卡**：千令牌主题编辑虚拟化（复用虚拟化）。
//! - **单源复用**：F3610 预算单源复用。
//! - **判据**：`run_ves29_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 50ms 超线→立案（复述红线实测） | [`V1DCode::EDIT_OVER`] |
//! | 批量超→优化（复述） | [`V1DCode::BATCH_OVER`] |
//! | 链爆→上限断言（红线实测） | [`V1DCode::CHAIN_OVERFLOW`] |
//! | 大主题卡→虚拟化（复用） | [`V1DCode::VIRTUAL_REQUIRED`] |
//!
//! **码段**：独占 **0x4dxx**。
//!
//! **性能逐项分解**：响应 O(1)；批量 O(n)；上限 O(1)；
//! 虚拟化 O(视口)。
//!
//! **跨批对接点**：F3610 单源复用；F2726 虚拟化复用；F3043 undo 复用
//! ——钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ── 预算常量 ────────────────────────────────────────────────────────

/// 编辑响应红线（ms）。
pub const EDIT_LIMIT_MS: u64 = 50;
/// 批量红线（ms）。
pub const BATCH_LIMIT_MS: u64 = 200;
/// undo 链上限（步数）。
pub const CHAIN_MAX: usize = 200;
/// 大主题阈值（令牌数）。
pub const LARGE_THEME_THRESHOLD: usize = 1000;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3629 诊断码段（独占 0x4dxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V1DCode(pub u16);

impl V1DCode {
    /// 编辑超线。
    pub const EDIT_OVER: V1DCode = V1DCode(0x4d01);
    /// 批量超线。
    pub const BATCH_OVER: V1DCode = V1DCode(0x4d02);
    /// undo 链爆。
    pub const CHAIN_OVERFLOW: V1DCode = V1DCode(0x4d03);
    /// 大主题需虚拟化。
    pub const VIRTUAL_REQUIRED: V1DCode = V1DCode(0x4d04);
    /// 零样本。
    pub const BAD_SAMPLE: V1DCode = V1DCode(0x4d05);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V1DCode::EDIT_OVER => {
                "编辑超 50ms 红线。建议：优化热点或启用代理。"
            }
            V1DCode::BATCH_OVER => {
                "批量超 200ms 红线。建议：拆批。"
            }
            V1DCode::CHAIN_OVERFLOW => {
                "undo 链爆上限（链内存红线）。建议：截断最旧步。"
            }
            V1DCode::VIRTUAL_REQUIRED => {
                "大主题需虚拟化。建议：启用虚拟化编辑列表。"
            }
            V1DCode::BAD_SAMPLE => {
                "零样本或异常。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 性能断言 ────────────────────────────────────────────────────────

/// 性能断言器。
#[derive(Clone, Debug, Default)]
pub struct PerfAssert;

impl PerfAssert {
    /// 编辑线断言。
    pub fn check_edit(ms: u64) -> Result<(), V1DCode> {
        if ms == 0 {
            return Err(V1DCode::BAD_SAMPLE);
        }
        if ms > EDIT_LIMIT_MS {
            Err(V1DCode::EDIT_OVER)
        } else {
            Ok(())
        }
    }

    /// 批量线断言。
    pub fn check_batch(ms: u64) -> Result<(), V1DCode> {
        if ms == 0 {
            return Err(V1DCode::BAD_SAMPLE);
        }
        if ms > BATCH_LIMIT_MS {
            Err(V1DCode::BATCH_OVER)
        } else {
            Ok(())
        }
    }
}

// ── undo 链上限 ─────────────────────────────────────────────────────

/// undo 链预算。
#[derive(Clone, Debug, Default)]
pub struct UndoBudget {
    stack: Vec<u64>,
}

impl UndoBudget {
    /// 新建。
    pub fn new() -> UndoBudget {
        UndoBudget { stack: Vec::new() }
    }

    /// 压入一步。超上限截断最旧。
    pub fn push(&mut self, snapshot: u64) -> Result<(), V1DCode> {
        if self.stack.len() >= CHAIN_MAX {
            self.stack.remove(0); // 截断最旧
            return Err(V1DCode::CHAIN_OVERFLOW);
        }
        self.stack.push(snapshot);
        Ok(())
    }

    /// 弹出一步。
    pub fn pop(&mut self) -> Option<u64> {
        self.stack.pop()
    }

    /// 深度。
    pub fn depth(&self) -> usize {
        self.stack.len()
    }
}

// ── 虚拟化列表（大主题）─────────────────────────────────────────────

/// 虚拟化视口。
#[derive(Clone, Debug)]
pub struct VirtualList {
    /// 总令牌数。
    pub total: usize,
    /// 视口大小。
    pub viewport: usize,
    /// 是否启用虚拟化。
    pub enabled: bool,
}

impl VirtualList {
    /// 新建（自动判定是否需要虚拟化）。
    pub fn new(total: usize, viewport: usize) -> VirtualList {
        VirtualList {
            total,
            viewport,
            enabled: total >= LARGE_THEME_THRESHOLD,
        }
    }

    /// 必要性审计：大主题必须启用虚拟化。
    pub fn audit(&self) -> Result<(), V1DCode> {
        if self.total >= LARGE_THEME_THRESHOLD && !self.enabled {
            Err(V1DCode::VIRTUAL_REQUIRED)
        } else {
            Ok(())
        }
    }

    /// 渲染项数（O(视口)）。
    pub fn render_count(&self) -> usize {
        if self.enabled {
            self.viewport.min(self.total)
        } else {
            self.total
        }
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3610", "VE-F2726", "VE-F3043"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves29_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3629");

    // 0x4d01 编辑线
    out.add(
        "0x4d01-编辑线",
        PerfAssert::check_edit(50).is_ok() && PerfAssert::check_edit(51).is_err(),
        "",
    );

    // 0x4d02 批量线
    out.add(
        "0x4d02-批量线",
        PerfAssert::check_batch(200).is_ok() && PerfAssert::check_batch(201).is_err(),
        "",
    );

    // 0x4d03 零样本
    out.add("0x4d03-零样本", PerfAssert::check_edit(0).is_err(), "");

    // 0x4d04 链上限
    let mut ub = UndoBudget::new();
    let mut overflow = false;
    for i in 0..CHAIN_MAX + 1 {
        if ub.push(i as u64).is_err() {
            overflow = true;
        }
    }
    out.add("0x4d04-链上限", overflow && ub.depth() <= CHAIN_MAX, "");

    // 0x4d05 弹出
    out.add("0x4d05-弹出", ub.pop().is_some(), "");

    // 0x4d06 大主题虚拟化
    let vl = VirtualList::new(2000, 50);
    out.add("0x4d06-大主题虚拟化", vl.enabled && vl.audit().is_ok(), "");

    // 0x4d07 虚拟化审计
    let vl2 = VirtualList {
        total: 2000,
        viewport: 50,
        enabled: false,
    };
    out.add("0x4d07-虚拟化审计", vl2.audit().is_err(), "");

    // 0x4d08 渲染项数
    out.add("0x4d08-渲染项数", vl.render_count() == 50, "");

    out
}
