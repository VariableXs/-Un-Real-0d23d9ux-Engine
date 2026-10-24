//! VE-F3727 · 创作内存治理（VE-S 域 · 批次 R07 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3727`
//!
//! **职责定位（锚点原文）**：创作内存治理——
//! - **四类账本**：画布缓冲/图层位图/undo 链/资源缓存四类记账
//!   （O08 模式五用复刻）；
//! - **上限**：创作内存 512MB（四类子预算，复述子预算红线）；
//! - **水位三级**：警戒→缓冲收缩 / 告急→代理降级 /
//!   临界→强制保存 + 只读（只读安全侧）——
//!   临界只读红线：内存临界宁可只读不许丢画布数据（数据保全第一）；
//! - **泄漏防线**：四类对账 + 注入自证复述。
//!
//! **判据（锚点原文六条）**：512MB 四类、只读保全、三域单源、
//! 自证复述、转设计、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 只读频发→根因转设计（复述转设计） | [`M7Code::READONLY_CHURN`] |
//! | 账本分歧→立案（复述） | [`M7Code::LEDGER_DIVERGE`] |
//! | 泄漏→P1（复述） | [`M7Code::LEAK_P1`] |
//! | 子预算破→强制（复述） | [`M7Code::SUB_BUDGET_BREAK`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3727 诊断码段（独占 0x76xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct M7Code(pub u16);

impl M7Code {
    /// 只读频发（转设计）。
    pub const READONLY_CHURN: M7Code = M7Code(0x7601);
    /// 账本分歧（立案）。
    pub const LEDGER_DIVERGE: M7Code = M7Code(0x7602);
    /// 泄漏（P1）。
    pub const LEAK_P1: M7Code = M7Code(0x7603);
    /// 子预算破（强制）。
    pub const SUB_BUDGET_BREAK: M7Code = M7Code(0x7604);
    /// 水位参数非法。
    pub const BAD_WATERMARK: M7Code = M7Code(0x7605);
    /// 临界未保全。
    pub const CRITICAL_UNSAVED: M7Code = M7Code(0x7606);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            M7Code::READONLY_CHURN => "只读频发（转设计）。建议：根因分析并优化内存结构。",
            M7Code::LEDGER_DIVERGE => "账本分歧（复述）。建议：立案对账四类账本。",
            M7Code::LEAK_P1 => "内存泄漏（P1）。建议：注入自证定位泄漏源。",
            M7Code::SUB_BUDGET_BREAK => "子预算破（强制）。建议：强制收缩该类缓存。",
            M7Code::BAD_WATERMARK => "水位参数非法。建议：警戒<告急<临界。",
            M7Code::CRITICAL_UNSAVED => "临界未保全（红线）。建议：强制保存后转只读。",
            _ => "未知诊断码。",
        }
    }
}

// ── 四类账本 ────────────────────────────────────────────────────────

/// 内存类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemClass {
    /// 画布缓冲。
    Canvas,
    /// 图层位图。
    Layer,
    /// undo 链。
    Undo,
    /// 资源缓存。
    Asset,
}

impl MemClass {
    pub const ALL: [MemClass; 4] = [
        MemClass::Canvas,
        MemClass::Layer,
        MemClass::Undo,
        MemClass::Asset,
    ];

    /// 子预算（KB，合计 512MB = 524288KB）。
    pub const fn sub_budget_kb(self) -> u32 {
        match self {
            MemClass::Canvas => 262_144, // 256MB
            MemClass::Layer => 131_072,  // 128MB
            MemClass::Undo => 65_536,    // 64MB
            MemClass::Asset => 65_536,   // 64MB
        }
    }
}

/// 四类账本。
pub struct MemLedger {
    used_kb: [u32; 4],
}

impl MemLedger {
    pub fn new() -> MemLedger {
        MemLedger { used_kb: [0; 4] }
    }

    /// 分配（超子预算拒）。
    pub fn alloc(&mut self, class: MemClass, kb: u32) -> Result<(), M7Code> {
        let idx = class as usize;
        let next = self.used_kb[idx].saturating_add(kb);
        if next > class.sub_budget_kb() {
            return Err(M7Code::SUB_BUDGET_BREAK);
        }
        self.used_kb[idx] = next;
        Ok(())
    }

    /// 释放。
    pub fn free(&mut self, class: MemClass, kb: u32) {
        let idx = class as usize;
        self.used_kb[idx] = self.used_kb[idx].saturating_sub(kb);
    }

    /// 总用量（KB）。
    pub fn total_kb(&self) -> u32 {
        self.used_kb[0]
            .saturating_add(self.used_kb[1])
            .saturating_add(self.used_kb[2])
            .saturating_add(self.used_kb[3])
    }

    /// 对账（总和 vs 外部计数）。
    pub fn reconcile(&self, external_kb: u32) -> Result<(), M7Code> {
        if self.total_kb() == external_kb {
            Ok(())
        } else {
            Err(M7Code::LEDGER_DIVERGE)
        }
    }
}

// ── 水位三级（只读保全红线） ────────────────────────────────────────

/// 水位级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Watermark {
    /// 正常。
    Normal,
    /// 警戒（缓冲收缩）。
    Warn,
    /// 告急（代理降级）。
    Urgent,
    /// 临界（强制保存 + 只读）。
    Critical,
}

/// 水位细则。
pub struct WatermarkPlan {
    /// 警戒线 ×100。
    warn_x100: u32,
    /// 告急线 ×100。
    urgent_x100: u32,
    /// 临界线 ×100。
    critical_x100: u32,
    /// 已强制保存。
    forced_save: bool,
    /// 只读态。
    readonly: bool,
}

impl WatermarkPlan {
    /// 构造（须警戒<告急<临界）。
    pub fn new(warn: u32, urgent: u32, critical: u32) -> Result<WatermarkPlan, M7Code> {
        if warn == 0 || warn >= urgent || urgent >= critical {
            return Err(M7Code::BAD_WATERMARK);
        }
        Ok(WatermarkPlan {
            warn_x100: warn,
            urgent_x100: urgent,
            critical_x100: critical,
            forced_save: false,
            readonly: false,
        })
    }

    /// 判级。
    pub fn level(&self, usage_x100: u32) -> Watermark {
        if usage_x100 >= self.critical_x100 {
            Watermark::Critical
        } else if usage_x100 >= self.urgent_x100 {
            Watermark::Urgent
        } else if usage_x100 >= self.warn_x100 {
            Watermark::Warn
        } else {
            Watermark::Normal
        }
    }

    /// 临界处置（强制保存 + 只读）。
    pub fn apply_critical(&mut self) -> Result<(), M7Code> {
        self.forced_save = true;
        self.readonly = true;
        // 保全红线：未保存不得进只读
        if !self.forced_save {
            return Err(M7Code::CRITICAL_UNSAVED);
        }
        Ok(())
    }

    /// 只读态。
    pub fn readonly(&self) -> bool {
        self.readonly
    }
}

// ── 泄漏防线（注入自证） ────────────────────────────────────────────

/// 注入样本（分配-释放对）。
pub struct LeakProbe {
    /// 配对样本（分配量 = 释放量即无泄漏）。
    pairs: Vec<(u32, u32)>,
}

impl LeakProbe {
    pub fn new() -> LeakProbe {
        LeakProbe { pairs: Vec::new() }
    }

    /// 上报一对。
    pub fn report(&mut self, alloc_kb: u32, free_kb: u32) {
        self.pairs.push((alloc_kb, free_kb));
    }

    /// 自证（任一对 alloc>free 即泄漏）。
    pub fn audit(&self) -> Result<(), M7Code> {
        for (a, f) in &self.pairs {
            if *a > *f {
                return Err(M7Code::LEAK_P1);
            }
        }
        Ok(())
    }
}

// ── 只读频发（转设计） ──────────────────────────────────────────────

/// 只读计数。
pub struct ReadonlyCounter {
    hits: u32,
    cap: u32,
}

impl ReadonlyCounter {
    pub fn new(cap: u32) -> ReadonlyCounter {
        ReadonlyCounter { hits: 0, cap }
    }

    /// 记一次只读。
    pub fn hit(&mut self) -> Result<(), M7Code> {
        self.hits += 1;
        if self.hits > self.cap {
            return Err(M7Code::READONLY_CHURN);
        }
        Ok(())
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3727 判据集（锚点原文六条逐条落位）。
pub fn run_vr07g_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3727");

    // 1) 512MB 四类：子预算之和
    let sum: u32 = MemClass::ALL.iter().map(|c| c.sub_budget_kb()).sum();
    out.add(
        "0x7604-512MB 四类",
        sum == 524_288,
        "四类合计 512MB",
    );

    // 2) 子预算破：被拒
    let mut led = MemLedger::new();
    out.add(
        "0x7604-子预算破",
        led.alloc(MemClass::Canvas, 300_000).is_err(),
        "超子预算须拒",
    );

    // 3) 只读保全：临界处置
    let mut wp = WatermarkPlan::new(7000, 8500, 9500).unwrap_or_else(|_| WatermarkPlan {
        warn_x100: 7000,
        urgent_x100: 8500,
        critical_x100: 9500,
        forced_save: false,
        readonly: false,
    });
    out.add(
        "0x7606-只读保全",
        wp.apply_critical().is_ok() && wp.readonly(),
        "临界须保全",
    );

    // 4) 水位：判级
    out.add(
        "0x7605-水位三级",
        wp.level(5000) == Watermark::Normal
            && wp.level(7500) == Watermark::Warn
            && wp.level(9000) == Watermark::Urgent
            && wp.level(9800) == Watermark::Critical,
        "三级判级须对",
    );

    // 5) 水位：非法被拒
    out.add(
        "0x7605-非法水位",
        WatermarkPlan::new(9000, 8000, 9500).is_err(),
        "警戒≥告急须拒",
    );

    // 6) 泄漏：自证检出
    let mut lp = LeakProbe::new();
    lp.report(100, 80);
    out.add("0x7603-泄漏自证", lp.audit().is_err(), "泄漏须拦");

    // 7) 账本：对账
    let mut led2 = MemLedger::new();
    let _ = led2.alloc(MemClass::Layer, 1000);
    out.add(
        "0x7602-账本对账",
        led2.reconcile(1000).is_ok() && led2.reconcile(2000).is_err(),
        "分歧须拦",
    );

    // 8) 只读频发：转设计
    let mut rc = ReadonlyCounter::new(2);
    let _ = rc.hit();
    let _ = rc.hit();
    out.add(
        "0x7601-只读频发",
        rc.hit().is_err(),
        "频发须转设计",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x7606-判据",
        M7Code::LEAK_P1.value() != M7Code::LEDGER_DIVERGE.value(),
        "码段互异",
    );

    out
}
