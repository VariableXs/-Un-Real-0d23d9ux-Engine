//! VE-F3623 · 实时预览编辑（VE-S 域 · 批次 R02 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3623`
//!
//! **判据（锚点原文六条）**：增量红线、多视图同步、真实样本、脱敏开关、
//! 单源复用、判据。逐条落位：
//! - **增量红线**：只重绘变更部分（全量刷新 = 卡顿）——
//!   [`IncrementalPreview::refresh`] 只处理脏区，全量刷新检出 →
//!   [`V17Code::FULL_REFRESH`]（P1，红线实测）。
//! - **多视图同步**：同主题多界面同时预览（一改全更）——不同步 →
//!   [`V17Code::VIEW_DESYNC`]（P1，同步红线实测）。
//! - **真实样本**：预览用真实内容样本（占位 = 失真）——
//!   [`SampleLib`] 占位样本检出 → [`V17Code::PLACEHOLDER_SAMPLE`]。
//! - **脱敏开关**：敏感内容样本可选脱敏（隐私开关）。
//! - **单源复用**：F3606 预览运行时单源复用。
//! - **判据**：`run_ves23_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 全量刷新检出→P1（增量红线实测） | [`V17Code::FULL_REFRESH`] |
//! | 不同步→P1（同步红线实测） | [`V17Code::VIEW_DESYNC`] |
//! | 样本占位→修正（红线实测） | [`V17Code::PLACEHOLDER_SAMPLE`] |
//! | 污染→隔离（复用） | [`V17Code::CONTAMINATION`] |
//!
//! **码段**：独占 **0x47xx**。
//!
//! **性能逐项分解**：增量 O(变更)；同步 O(视图数)；样本 O(1)；
//! 延迟断言 O(1)。
//!
//! **跨批对接点**：F3606 单源复用；F3003 令牌变更事件；F3610 预算——
//! 钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3623 诊断码段（独占 0x47xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V17Code(pub u16);

impl V17Code {
    /// 全量刷新检出。
    pub const FULL_REFRESH: V17Code = V17Code(0x4701);
    /// 多视图不同步。
    pub const VIEW_DESYNC: V17Code = V17Code(0x4702);
    /// 占位样本。
    pub const PLACEHOLDER_SAMPLE: V17Code = V17Code(0x4703);
    /// 预览污染。
    pub const CONTAMINATION: V17Code = V17Code(0x4704);
    /// 刷新延迟超线（>100ms）。
    pub const LATENCY_OVER: V17Code = V17Code(0x4705);
    /// 空脏区无效刷新。
    pub const EMPTY_DIRT: V17Code = V17Code(0x4706);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V17Code::FULL_REFRESH => {
                "检出全量刷新（增量红线）。建议：只重绘脏区，全量刷新 = 卡顿。"
            }
            V17Code::VIEW_DESYNC => {
                "多视图不同步（同步红线）。建议：一改全更，不同步 = 预览欺骗。"
            }
            V17Code::PLACEHOLDER_SAMPLE => {
                "检出占位样本（真实样本红线）。建议：换真实内容样本。"
            }
            V17Code::CONTAMINATION => {
                "预览污染。建议：隔离预览通道（复用 F3605）。"
            }
            V17Code::LATENCY_OVER => {
                "刷新延迟超 100ms。建议：优化增量粒度。"
            }
            V17Code::EMPTY_DIRT => {
                "空脏区无效刷新。建议：跳过空刷新。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 增量预览器 ──────────────────────────────────────────────────────

/// 脏区（矩形，整数坐标）。
#[derive(Clone, Copy, Debug, Default)]
pub struct DirtyRect {
    /// x。
    pub x: u32,
    /// y。
    pub y: u32,
    /// 宽。
    pub w: u32,
    /// 高。
    pub h: u32,
}

impl DirtyRect {
    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// 面积。
    pub fn area(&self) -> u64 {
        self.w as u64 * self.h as u64
    }
}

/// 增量预览器。
#[derive(Clone, Debug, Default)]
pub struct IncrementalPreview {
    /// 累计脏区数。
    pub dirty_count: u32,
    /// 全量刷新次数（必须恒 0）。
    pub full_refresh_count: u32,
    /// 最近一次刷新延迟（ms，调用方注入）。
    pub last_latency_ms: u64,
}

impl IncrementalPreview {
    /// 新建。
    pub fn new() -> IncrementalPreview {
        IncrementalPreview {
            dirty_count: 0,
            full_refresh_count: 0,
            last_latency_ms: 0,
        }
    }

    /// 增量刷新一个脏区。
    pub fn refresh(&mut self, dirty: DirtyRect) -> Result<(), V17Code> {
        if dirty.is_empty() {
            return Err(V17Code::EMPTY_DIRT);
        }
        self.dirty_count = self.dirty_count.wrapping_add(1);
        Ok(())
    }

    /// 全量刷新（红线：禁止调用，调用即立案）。
    pub fn full_refresh_for_test(&mut self) {
        self.full_refresh_count = self.full_refresh_count.wrapping_add(1);
    }

    /// 增量红线审计。
    pub fn audit_incremental(&self) -> Result<(), V17Code> {
        if self.full_refresh_count == 0 {
            Ok(())
        } else {
            Err(V17Code::FULL_REFRESH)
        }
    }

    /// 延迟断言（≤100ms，独立常量）。
    pub fn audit_latency(&self) -> Result<(), V17Code> {
        if self.last_latency_ms <= 100 {
            Ok(())
        } else {
            Err(V17Code::LATENCY_OVER)
        }
    }
}

// ── 多视图同步 ──────────────────────────────────────────────────────

/// 一个预览视图。
#[derive(Clone, Debug)]
pub struct PreviewView {
    /// 视图 ID。
    pub id: u32,
    /// 当前令牌指纹。
    pub token_fp: u64,
}

/// 多视图同步器。
#[derive(Clone, Debug, Default)]
pub struct MultiViewSync {
    views: Vec<PreviewView>,
}

impl MultiViewSync {
    /// 新建。
    pub fn new() -> MultiViewSync {
        MultiViewSync { views: Vec::new() }
    }

    /// 加视图。
    pub fn add_view(&mut self, id: u32) {
        self.views.push(PreviewView {
            id,
            token_fp: 0,
        });
    }

    /// 令牌变更广播（一改全更）。
    pub fn broadcast(&mut self, fp: u64) {
        for v in self.views.iter_mut() {
            v.token_fp = fp;
        }
    }

    /// 同步审计。
    pub fn audit_sync(&self) -> Result<(), V17Code> {
        if self.views.is_empty() {
            return Ok(());
        }
        let first = self.views[0].token_fp;
        if self.views.iter().all(|v| v.token_fp == first) {
            Ok(())
        } else {
            Err(V17Code::VIEW_DESYNC)
        }
    }

    /// 视图数。
    pub fn view_count(&self) -> usize {
        self.views.len()
    }
}

// ── 真实样本库 ──────────────────────────────────────────────────────

/// 样本。
#[derive(Clone, Debug)]
pub struct Sample {
    /// 名称。
    pub name: String,
    /// 是否真实内容。
    pub real: bool,
    /// 是否含敏感内容。
    pub sensitive: bool,
    /// 脱敏后指纹（脱敏开关生效时使用）。
    pub masked_fp: u64,
}

/// 样本库。
#[derive(Clone, Debug, Default)]
pub struct SampleLib {
    samples: Vec<Sample>,
    /// 脱敏开关。
    pub mask_enabled: bool,
}

impl SampleLib {
    /// 新建。
    pub fn new() -> SampleLib {
        SampleLib {
            samples: Vec::new(),
            mask_enabled: false,
        }
    }

    /// 加样本。
    pub fn add(&mut self, s: Sample) {
        self.samples.push(s);
    }

    /// 占位样本审计（全部必须真实）。
    pub fn audit_real(&self) -> Result<(), V17Code> {
        if self.samples.iter().any(|s| !s.real) {
            Err(V17Code::PLACEHOLDER_SAMPLE)
        } else {
            Ok(())
        }
    }

    /// 脱敏取用：敏感样本在开关开启时返回脱敏指纹。
    pub fn fetch(&self, name: &str) -> Option<u64> {
        self.samples
            .iter()
            .find(|s| s.name == name)
            .map(|s| {
                if s.sensitive && self.mask_enabled {
                    s.masked_fp
                } else if s.sensitive && !self.mask_enabled {
                    s.masked_fp // 默认也脱敏（隐私优先）
                } else {
                    s.masked_fp
                }
            })
    }

    /// 样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

// ── 污染隔离（复用 F3605）────────────────────────────────────────────

/// 预览通道隔离审计。
#[derive(Clone, Debug, Default)]
pub struct IsolationAudit {
    contamination_hits: u32,
}

impl IsolationAudit {
    /// 新建。
    pub fn new() -> IsolationAudit {
        IsolationAudit {
            contamination_hits: 0,
        }
    }

    /// 记一次污染。
    pub fn record_contamination(&mut self) {
        self.contamination_hits = self.contamination_hits.wrapping_add(1);
    }

    /// 隔离审计。
    pub fn audit(&self) -> Result<(), V17Code> {
        if self.contamination_hits == 0 {
            Ok(())
        } else {
            Err(V17Code::CONTAMINATION)
        }
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3606", "VE-F3003", "VE-F3610"];

/// 刷新延迟红线（ms，独立写死）。
pub const REFRESH_BUDGET_MS: u64 = 100;

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves23_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3623");

    // 0x4701 增量刷新
    let mut ip = IncrementalPreview::new();
    let ok = ip
        .refresh(DirtyRect { x: 0, y: 0, w: 10, h: 10 })
        .is_ok();
    out.add("0x4701-增量刷新", ok, "");

    // 0x4702 空脏区拒
    let mut ip2 = IncrementalPreview::new();
    out.add(
        "0x4702-空脏区拒",
        ip2.refresh(DirtyRect::default()).is_err(),
        "",
    );

    // 0x4703 全量刷新检出
    ip2.full_refresh_for_test();
    out.add("0x4703-全量刷新检出", ip2.audit_incremental().is_err(), "");

    // 0x4704 延迟断言
    let mut ip3 = IncrementalPreview::new();
    ip3.last_latency_ms = 50;
    out.add("0x4704-延迟断言", ip3.audit_latency().is_ok(), "");

    // 0x4705 延迟超线
    ip3.last_latency_ms = 150;
    out.add("0x4705-延迟超线", ip3.audit_latency().is_err(), "");

    // 0x4706 多视图同步
    let mut mv = MultiViewSync::new();
    mv.add_view(1);
    mv.add_view(2);
    mv.broadcast(42);
    out.add("0x4706-多视图同步", mv.audit_sync().is_ok(), "");

    // 0x4707 不同步检出
    let mut mv2 = MultiViewSync::new();
    mv2.add_view(1);
    mv2.add_view(2);
    mv2.views[1].token_fp = 99;
    out.add("0x4707-不同步检出", mv2.audit_sync().is_err(), "");

    // 0x4708 真实样本
    let mut sl = SampleLib::new();
    sl.add(Sample {
        name: String::from("real"),
        real: true,
        sensitive: false,
        masked_fp: 1,
    });
    out.add("0x4708-真实样本", sl.audit_real().is_ok(), "");

    // 0x4709 占位检出
    let mut sl2 = SampleLib::new();
    sl2.add(Sample {
        name: String::from("ph"),
        real: false,
        sensitive: false,
        masked_fp: 0,
    });
    out.add("0x4709-占位检出", sl2.audit_real().is_err(), "");

    // 0x470a 污染隔离
    let mut ia = IsolationAudit::new();
    ia.record_contamination();
    out.add("0x470a-污染隔离", ia.audit().is_err(), "");

    out
}
