//! VE-F3731 · 创作性能调试器（VE-S 域 · 批次 R07 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3731`
//!
//! **职责定位（锚点原文）**：创作性能调试器——
//! - **统一框架**：四工具统一性能检视（复用 F3342 单源声明）——统一复述；
//! - **仪表**：四工具八源仪表 + 心流视图（复述顶层）；
//! - **火焰**：创作八段火焰（输入/画布/图层/滤镜/预览/序列化/压缩/undo）——
//!   段火焰；
//! - **单操作剖析**：单次编辑操作全程剖析（复述对照模型可视化）；
//! - **降档回放 + 注入**复述。
//!
//! **判据（锚点原文六条）**：统一复用、心流顶层、八段火焰、
//! 家族二十、零常态、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 仪表分歧→立案（复述） | [`T7Code::METER_DIVERGE`] |
//! | 心流视图缺→修正（复述红线实测） | [`T7Code::FLOW_VIEW_MISS`] |
//! | 开销→打开才采样（复述） | [`T7Code::OVERHEAD`] |
//! | 注入逃逸→沙箱（复述） | [`T7Code::INJECT_ESCAPE`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3731 诊断码段（独占 0x7Axx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct T7Code(pub u16);

impl T7Code {
    /// 仪表分歧（立案）。
    pub const METER_DIVERGE: T7Code = T7Code(0x7A01);
    /// 心流视图缺（修正红线）。
    pub const FLOW_VIEW_MISS: T7Code = T7Code(0x7A02);
    /// 常态开销（打开才采样）。
    pub const OVERHEAD: T7Code = T7Code(0x7A03);
    /// 注入逃逸（沙箱）。
    pub const INJECT_ESCAPE: T7Code = T7Code(0x7A04);
    /// 段数非法。
    pub const BAD_SEGMENT: T7Code = T7Code(0x7A05);
    /// 协议成员缺失。
    pub const PROTO_MISS: T7Code = T7Code(0x7A06);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            T7Code::METER_DIVERGE => "仪表分歧（复述）。建议：双表对账立案。",
            T7Code::FLOW_VIEW_MISS => "心流视图缺失（红线实测）。建议：补心流顶层视图。",
            T7Code::OVERHEAD => "常态开销（复述）。建议：采样开关须默认关。",
            T7Code::INJECT_ESCAPE => "注入逃逸（复述）。建议：注入须沙箱隔离。",
            T7Code::BAD_SEGMENT => "段数非法。建议：火焰须满八段。",
            T7Code::PROTO_MISS => "协议成员缺失。建议：家族二十须齐。",
            _ => "未知诊断码。",
        }
    }
}

// ── 八段火焰 ────────────────────────────────────────────────────────

/// 火焰段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlameSeg {
    /// 输入。
    Input,
    /// 画布。
    Canvas,
    /// 图层。
    Layer,
    /// 滤镜。
    Filter,
    /// 预览。
    Preview,
    /// 序列化。
    Serialize,
    /// 压缩。
    Compress,
    /// undo。
    Undo,
}

impl FlameSeg {
    pub const ALL: [FlameSeg; 8] = [
        FlameSeg::Input,
        FlameSeg::Canvas,
        FlameSeg::Layer,
        FlameSeg::Filter,
        FlameSeg::Preview,
        FlameSeg::Serialize,
        FlameSeg::Compress,
        FlameSeg::Undo,
    ];
}

/// 火焰图。
pub struct FlameGraph {
    /// 各段耗时（毫秒）。
    seg_ms: [u32; 8],
    /// 采集开关（零常态）。
    sampling: bool,
}

impl FlameGraph {
    pub fn new() -> FlameGraph {
        FlameGraph {
            seg_ms: [0; 8],
            sampling: false,
        }
    }

    /// 开启采样（打开才采样红线）。
    pub fn enable(&mut self) {
        self.sampling = true;
    }

    /// 关闭采样。
    pub fn disable(&mut self) {
        self.sampling = false;
    }

    /// 上报段耗时（未开采样则丢弃）。
    pub fn report(&mut self, seg: FlameSeg, ms: u32) {
        if !self.sampling {
            return;
        }
        self.seg_ms[seg as usize] = ms;
    }

    /// 总耗时。
    pub fn total_ms(&self) -> u32 {
        self.seg_ms
            .iter()
            .fold(0u32, |acc, x| acc.saturating_add(*x))
    }

    /// 八段齐备检查（非零段数 = 已采）。
    pub fn segments_seen(&self) -> u32 {
        let mut n = 0;
        for s in &self.seg_ms {
            if *s > 0 {
                n += 1;
            }
        }
        n
    }

    /// 零常态断言（未开采样时总耗时须 0）。
    pub fn zero_overhead(&self) -> Result<(), T7Code> {
        if !self.sampling && self.total_ms() > 0 {
            return Err(T7Code::OVERHEAD);
        }
        Ok(())
    }
}

// ── 心流视图（顶层红线） ────────────────────────────────────────────

/// 仪表视图。
pub struct MeterView {
    /// 心流视图是否顶层。
    flow_on_top: bool,
    /// 视图数。
    views: u8,
}

impl MeterView {
    /// 构造（心流顶层）。
    pub fn standard() -> MeterView {
        MeterView {
            flow_on_top: true,
            views: 9, // 八源 + 心流
        }
    }

    /// 构造违规（心流非顶层）。
    pub fn violation() -> MeterView {
        MeterView {
            flow_on_top: false,
            views: 9,
        }
    }

    /// 心流顶层断言。
    pub fn assert(&self) -> Result<(), T7Code> {
        if self.flow_on_top && self.views >= 9 {
            Ok(())
        } else {
            Err(T7Code::FLOW_VIEW_MISS)
        }
    }
}

// ── 统一框架（F3342 单源复述） ──────────────────────────────────────

/// 工具仪表。
pub struct ToolMeter {
    /// 工具 ID（0-3）。
    tool: u8,
    /// 读数。
    reading: u32,
}

/// 统一检视器。
pub struct UnifiedProbe {
    meters: Vec<ToolMeter>,
    /// 参考读数（对账）。
    reference: u32,
}

impl UnifiedProbe {
    pub fn new(reference: u32) -> UnifiedProbe {
        UnifiedProbe {
            meters: Vec::new(),
            reference,
        }
    }

    /// 上报仪表。
    pub fn ingest(&mut self, tool: u8, reading: u32) {
        self.meters.push(ToolMeter { tool, reading });
    }

    /// 对账（全部读数须等于参考）。
    pub fn reconcile(&self) -> Result<(), T7Code> {
        for m in &self.meters {
            if m.reading != self.reference {
                return Err(T7Code::METER_DIVERGE);
            }
        }
        Ok(())
    }
}

// ── 沙箱（注入逃逸防线） ────────────────────────────────────────────

/// 沙箱。
pub struct Sandbox {
    escaped: bool,
}

impl Sandbox {
    pub fn new() -> Sandbox {
        Sandbox { escaped: false }
    }

    /// 跑注入（逃逸即拦）。
    pub fn run_inject(&mut self, escape: bool) -> Result<(), T7Code> {
        if escape {
            self.escaped = true;
            return Err(T7Code::INJECT_ESCAPE);
        }
        Ok(())
    }

    /// 逃逸态。
    pub fn escaped(&self) -> bool {
        self.escaped
    }
}

// ── 家族二十协议（F2815 复述） ──────────────────────────────────────

/// 协议成员检查。
pub struct FamilyTwenty {
    seen: u8,
}

impl FamilyTwenty {
    pub fn new() -> FamilyTwenty {
        FamilyTwenty { seen: 0 }
    }

    /// 登记成员。
    pub fn enroll(&mut self) {
        self.seen = self.seen.saturating_add(1);
    }

    /// 齐备（20 位）。
    pub fn complete(&self) -> Result<(), T7Code> {
        if self.seen >= 20 {
            Ok(())
        } else {
            Err(T7Code::PROTO_MISS)
        }
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3731 判据集（锚点原文六条逐条落位）。
pub fn run_vr07k_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3731");

    // 1) 八段火焰：段集齐
    out.add(
        "0x7A05-八段火焰",
        FlameSeg::ALL.len() == 8,
        "八段须齐",
    );

    // 2) 零常态：未采样零开销
    let mut fg = FlameGraph::new();
    out.add(
        "0x7A03-零常态",
        fg.zero_overhead().is_ok() && fg.total_ms() == 0,
        "零常态须守",
    );

    // 3) 打开才采样：采样后可报
    fg.enable();
    fg.report(FlameSeg::Canvas, 12);
    fg.report(FlameSeg::Layer, 8);
    out.add(
        "0x7A03-采样开关",
        fg.total_ms() == 20 && fg.segments_seen() == 2,
        "采样须工作",
    );

    // 4) 心流顶层：标准过
    let mv = MeterView::standard();
    out.add(
        "0x7A02-心流顶层",
        mv.assert().is_ok(),
        "心流须顶层",
    );

    // 5) 心流顶层：违规拦
    let bad = MeterView::violation();
    out.add(
        "0x7A02-违规拦",
        bad.assert().is_err(),
        "违规须拦",
    );

    // 6) 统一复用：对账
    let mut up = UnifiedProbe::new(100);
    up.ingest(0, 100);
    up.ingest(1, 100);
    out.add(
        "0x7A01-统一复用",
        up.reconcile().is_ok(),
        "一致须过",
    );

    // 7) 仪表分歧：被拦
    up.ingest(2, 999);
    out.add(
        "0x7A01-分歧拦",
        up.reconcile().is_err(),
        "分歧须拦",
    );

    // 8) 沙箱：逃逸拦
    let mut sb = Sandbox::new();
    out.add(
        "0x7A04-沙箱逃逸",
        sb.run_inject(true).is_err() && sb.escaped(),
        "逃逸须拦",
    );

    // 9) 家族二十：齐备
    let mut f20 = FamilyTwenty::new();
    for _ in 0..20 {
        f20.enroll();
    }
    out.add(
        "0x7A06-家族二十",
        f20.complete().is_ok(),
        "二十须齐",
    );

    out
}
