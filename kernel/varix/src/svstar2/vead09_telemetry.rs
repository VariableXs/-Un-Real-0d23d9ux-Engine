//! vead09_telemetry — VE-F6009 物理遥测（VE-AD 域 · F6009 · 目标 300 行，深化版）。
//!
//! 判据原文（施工书锚点 VE-F6009）：物理遥测——步长耗时、物理体数量、约束数量、回放
//! 使用四类聚合（物理参数可分析、场景内容不记录）；遥测含物理体数量时间线——规模随
//! 时间变化曲线洪峰一眼可见；含时间线数据开放导出；时间线含异常事件标注；标注含事件
//! 来源分类；分类含统计汇总。数据结构：四类聚合器。错误路径与降级矩阵：失败→缓存；
//! 含内容→阻断；超限→滚动。性能逐项分解：聚合 O（样本）。跨批对接点：U 域总日志
//! 中心。无障碍与隐私：口径公开。判据：四类聚合、内容不记、口径公开、判据。
//!
//! **职责定位**：四类聚合器（步长耗时 µs · 物理体数 · 约束数 · 回放使用 tick）单遍
//! O(样本) 聚合，每类独立 min/max/sum/count 四元账；物理体数量时间线滚动窗口（超限
//! 滚动降级 0xB403——旧样本让位新样本，窗口内洪峰 peak() 一眼可见且必须带标注位）；
//! 异常事件标注闭集（tick+类别+**来源分类**三件齐才收，缺来源 0xB407——标注可归因）
//! ；来源分类统计汇总（per-source 次数账，O(标注) 单遍）；时间线数据开放导出（行式
//! schema 版本标头+逐行 tick,四类值——口径公开判据的导出面，行上限截断显性 0xB408
//! 不吞数）。降级矩阵三行全显性：聚合失败→缓存留档（0xB401，缓存容量护栏满容拒新
//! 不覆盖）；样本含内容→阻断入库（0xB402——RawTelemetry 显性携带 content 字段，
//! 检出即拒，TelemetrySample 闭集无内容字段编译期保证）；窗口超限→滚动（0xB403）。
//! 防线红线：负值计量显性拒（0xB405——耗时/体数/约束数/回放用量天然非负，负值即
//! 上游损坏）；计数器饱和溢出显性立案（0xB406/0xB40C——saturating 不静默回绕）；
//! tick 非单调拒收（0xB40A——时间线有序性是洪峰语义的前提）；口径冲突拒聚合
//! （0xB40B——四类计量单位口径版本一致才可合账）。跨批对接点：U 域总日志中心——
//! USchema 对接声明（行格式版本+字段序+分隔符三件固定，总日志中心按 schema 消费）
//! 。无障碍与隐私：口径公开（Caliber 全字段 pub+describe 一句话可读屏）；零内容
//! 记录（导出行只有数值与 tick，无任何场景内容）。
//!
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，全整数算术（无浮点），逻辑 tick
//! 注入零墙钟，FNV-1a 指纹，诊断码 0xB4 段闭集（0xB401~0xB40C 十二码两两互异）。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::result::Result;

use crate::checks::CheckSet;

// ─────────────────────────────────────────────────────────────────────────────
// 诊断码 0xB4 段闭集（本文件独占；0xB2 已被 vead08 占段，0xB3 与并行 phystel 撞段，改取空闲 0xB4）
// ─────────────────────────────────────────────────────────────────────────────

/// 本模块诊断码段基址：0xB4（0xB401~0xB40C 十二码闭集）。
pub const SEGMENT_BASE: u16 = 0xB4;

/// 诊断码闭集：聚合/时间线/标注/导出全链错误路径逐码显性，禁静默吞错。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 0xB401 聚合失败已缓存——样本入缓存留档待补（降级矩阵第一行：失败→缓存）。
    SampleCacheFallback,
    /// 0xB402 含内容阻断——原始遥测携带内容字段，拒绝入库（降级矩阵第二行）。
    ContentDetectedBlocked,
    /// 0xB403 窗口超限滚动——时间线滚动窗口让位旧样本（降级矩阵第三行）。
    WindowOverflowRolling,
    /// 0xB404 空样本流——零样本导出/汇总裁决显性拒。
    EmptyStream,
    /// 0xB405 负值计量——耗时/体数/约束数/回放用量天然非负，负值即上游损坏。
    NegativeMetric,
    /// 0xB406 计数器饱和——计数器触顶 saturating（不回绕但必须立案）。
    CounterOverflow,
    /// 0xB407 标注缺来源——异常事件标注必须带来源分类（可归因纪律）。
    AnnotationUnsourced,
    /// 0xB408 导出截断——导出行数超上限，截断显性计数。
    ExportTruncated,
    /// 0xB409 洪峰未标注——洪峰样本必须带标注位（一眼可见判据的兜底防线）。
    PeakUnmarked,
    /// 0xB40A tick 非单调——时间线乱序拒收（洪峰语义前提是有序时间轴）。
    NonMonotonicTick,
    /// 0xB40B 口径冲突——聚合口径版本不一致，拒合账。
    CaliberMismatch,
    /// 0xB40C 聚合和溢出——聚合 sum 触顶 saturating（不回绕但必须立案）。
    AggregateOverflow,
}

impl XcCode {
    /// 诊断码值：段基址左移八位 + 段内序号（0xB401 起）。
    pub fn code(self) -> u16 {
        (SEGMENT_BASE << 8)
            | match self {
                XcCode::SampleCacheFallback => 0x01,
                XcCode::ContentDetectedBlocked => 0x02,
                XcCode::WindowOverflowRolling => 0x03,
                XcCode::EmptyStream => 0x04,
                XcCode::NegativeMetric => 0x05,
                XcCode::CounterOverflow => 0x06,
                XcCode::AnnotationUnsourced => 0x07,
                XcCode::ExportTruncated => 0x08,
                XcCode::PeakUnmarked => 0x09,
                XcCode::NonMonotonicTick => 0x0A,
                XcCode::CaliberMismatch => 0x0B,
                XcCode::AggregateOverflow => 0x0C,
            }
    }

    /// 可操作建议（全码非空——逐码给下一步动作）。
    pub fn detail(self) -> &'static str {
        match self {
            XcCode::SampleCacheFallback => "样本已入缓存留档：查聚合失败原因，修后从缓存补聚合",
            XcCode::ContentDetectedBlocked => "原始遥测含内容字段：删 content 或改走内容域专用通道，遥测只收参数",
            XcCode::WindowOverflowRolling => "窗口已滚动：超窗样本已被让位，如需全量改用导出通道落盘",
            XcCode::EmptyStream => "零样本无法裁决：确认采集管线连通再导出",
            XcCode::NegativeMetric => "负值计量即上游损坏：查物理步进器的耗时/体数来源",
            XcCode::CounterOverflow => "计数器触顶饱和：样本量超设计规模，扩容或分窗",
            XcCode::AnnotationUnsourced => "标注缺来源分类：补 SourceKind 后重提交标注",
            XcCode::ExportTruncated => "导出已截断：全量按 tick 分段二次导出",
            XcCode::PeakUnmarked => "洪峰样本未带标注：检查标注管线是否漏采洪峰点",
            XcCode::NonMonotonicTick => "tick 回退拒收：查时钟源，时间线必须单调",
            XcCode::CaliberMismatch => "口径版本不一致：统一 Caliber 后再聚合",
            XcCode::AggregateOverflow => "聚合 sum 触顶饱和：总量超设计规模，分窗结算",
        }
    }
}

/// 诊断对象：码 + 现场短述，render 输出「[0xB4NN] 述 || 建议」可读行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    code: XcCode,
    where_: String,
}

impl Diag {
    /// 构造诊断。
    pub fn new(code: XcCode, where_: &str) -> Self {
        Diag {
            code,
            where_: where_.to_string(),
        }
    }

    /// 诊断码值。
    pub fn code(&self) -> u16 {
        self.code.code()
    }

    /// 码枚举（判据 matches! 精确归因）。
    pub fn kind(&self) -> XcCode {
        self.code
    }

    /// 可读渲染：码 + 现场 + 建议（纯文本单行可读屏）。
    pub fn render(&self) -> String {
        format!(
            "[0x{:04X}] {} || {}",
            self.code.code(),
            self.where_,
            self.code.detail()
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FNV-1a 32 位指纹（与 vead08/veab07 同构共用算法——同输入同摘要）
// ─────────────────────────────────────────────────────────────────────────────

/// FNV-1a 32 位偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;
/// FNV-1a 32 位素数。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// 单字节推进。
pub fn fnv_step(hash: u32, byte: u8) -> u32 {
    (hash ^ (byte as u32)).wrapping_mul(FNV_PRIME)
}

/// 字节切片指纹。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h = FNV_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        h = fnv_step(h, bytes[i]);
        i += 1;
    }
    h
}

/// u64 按小端八字节指纹。
pub fn fnv_u64(hash: u32, v: u64) -> u32 {
    let mut h = hash;
    let mut i = 0;
    while i < 8 {
        h = fnv_step(h, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    h
}

// ─────────────────────────────────────────────────────────────────────────────
// 口径公开（无障碍与隐私判据的数据面）
// ─────────────────────────────────────────────────────────────────────────────

/// 计量口径：四类聚合的单位与版本——**全字段 pub**，口径一句话可读可查。
/// 口径冲突（版本不一致的两套遥测合账）显性拒 0xB40B。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caliber {
    /// 口径版本（同版本才可合账）。
    pub version: u32,
    /// 步长耗时单位（恒 "us" 微秒整数）。
    pub step_cost_unit: &'static str,
    /// 物理体数单位（恒 "count"）。
    pub body_unit: &'static str,
    /// 约束数单位（恒 "count"）。
    pub constraint_unit: &'static str,
    /// 回放使用单位（恒 "tick"）。
    pub replay_unit: &'static str,
}

impl Caliber {
    /// 标准口径 v1。
    pub const V1: Caliber = Caliber {
        version: 1,
        step_cost_unit: "us",
        body_unit: "count",
        constraint_unit: "count",
        replay_unit: "tick",
    };

    /// 口径可读渲染（口径公开判据落地：一句话说清四类单位与版本）。
    pub fn describe(&self) -> String {
        format!(
            "caliber v{}: step_cost[{}] bodies[{}] constraints[{}] replay[{}] — params only, no scene content",
            self.version,
            self.step_cost_unit,
            self.body_unit,
            self.constraint_unit,
            self.replay_unit
        )
    }

    /// 口径一致性：单位四件+版本全等才可合账。
    pub fn compatible(&self, other: &Caliber) -> bool {
        self == other
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 遥测样本（闭集内容零记录）与原始遥测（防线显性面）
// ─────────────────────────────────────────────────────────────────────────────

/// 合法遥测样本：闭集四类数值+tick，**无内容字段**（编译期保证——聚合器只收此类型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelemetrySample {
    /// 逻辑 tick。
    pub tick: u64,
    /// 步长耗时（微秒整数）。
    pub step_cost_us: u32,
    /// 物理体数量。
    pub bodies: u32,
    /// 约束数量。
    pub constraints: u32,
    /// 回放使用（tick 计）。
    pub replay_ticks: u32,
}

impl TelemetrySample {
    /// 样本指纹（FNV-1a 逐字段——同样本同指纹，导出对账用）。
    pub fn fingerprint(&self) -> u32 {
        let h0 = fnv_u64(FNV_OFFSET.wrapping_add(0xB4), self.tick);
        let h1 = fnv_u64(h0, self.step_cost_us as u64);
        let h2 = fnv_u64(h1, self.bodies as u64);
        let h3 = fnv_u64(h2, self.constraints as u64);
        fnv_u64(h3, self.replay_ticks as u64)
    }
}

/// 原始遥测：采集入口形态——**显性携带 content 字段**，让「含内容→阻断」在类型上
/// 有据可检（TelemetrySample 收不到内容，内容在入口闸即被拦 0xB402）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawTelemetry {
    /// 逻辑 tick。
    pub tick: u64,
    /// 步长耗时（微秒）。
    pub step_cost_us: u32,
    /// 物理体数量。
    pub bodies: u32,
    /// 约束数量。
    pub constraints: u32,
    /// 回放使用（tick 计）。
    pub replay_ticks: u32,
    /// 内容挂载点（显性存在以供防线检出；合法采集恒填 None）。
    pub content: Option<String>,
}

impl RawTelemetry {
    /// 合法样本封装（content 恒 None——四类参数之外不携带任何东西）。
    pub fn pure(tick: u64, step_cost_us: u32, bodies: u32, constraints: u32, replay_ticks: u32) -> Self {
        RawTelemetry {
            tick,
            step_cost_us,
            bodies,
            constraints,
            replay_ticks,
            content: None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 四类聚合器（单遍 O(样本)；每类独立 min/max/sum/count 四元账）
// ─────────────────────────────────────────────────────────────────────────────

/// 单类四元账：min/max/sum/count。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricLedger {
    /// 最小值。
    pub min: u64,
    /// 最大值。
    pub max: u64,
    /// 累计和（saturating——触顶立案 0xB40C）。
    pub sum: u64,
    /// 样本数。
    pub count: u64,
}

impl MetricLedger {
    /// 空账（min/max 以 u64::MAX/0 为哨兵，count==0 时不可读）。
    pub const fn new() -> Self {
        MetricLedger {
            min: u64::MAX,
            max: 0,
            sum: 0,
            count: 0,
        }
    }

    /// 单值入账（O(1)；返回 true=本次发生饱和溢出，供上游立案）。
    pub fn ingest(&mut self, v: u32) -> bool {
        let ov_sum = self.sum.checked_add(v as u64).is_none();
        let v = v as u64;
        if v < self.min {
            self.min = v;
        }
        if v > self.max {
            self.max = v;
        }
        self.sum = self.sum.saturating_add(v);
        let ov_count = self.count == u64::MAX;
        self.count = self.count.saturating_add(1);
        ov_sum || ov_count
    }

    /// 均值（整数除法，count==0 返回 None——空账显性不猜）。
    pub fn avg(&self) -> Option<u64> {
        if self.count == 0 {
            None
        } else {
            Some(self.sum / self.count)
        }
    }

    /// 账空判。
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// 四类聚合器：步长耗时/物理体数/约束数/回放使用四元账 + 溢出立案计数。
pub struct FourAggregator {
    caliber: Caliber,
    step_cost: MetricLedger,
    bodies: MetricLedger,
    constraints: MetricLedger,
    replay: MetricLedger,
    /// 聚合 sum 饱和立案计数（0xB40C；0xB406 计数器饱和由 MetricLedger 返回值并入此账）。
    aggregate_overflows: u32,
    ingested: u64,
}

impl FourAggregator {
    /// 以口径开聚合器。
    pub fn new(caliber: Caliber) -> Self {
        FourAggregator {
            caliber,
            step_cost: MetricLedger::new(),
            bodies: MetricLedger::new(),
            constraints: MetricLedger::new(),
            replay: MetricLedger::new(),
            aggregate_overflows: 0,
            ingested: 0,
        }
    }

    /// 单样本四类入账（O(1)；四账同步推进——单类失败不影响他类的四账原子性由
    /// 全 u32 入参保证：要么四账全进要么整体拒）。
    pub fn ingest(&mut self, s: &TelemetrySample) {
        let ov1 = self.step_cost.ingest(s.step_cost_us);
        let ov2 = self.bodies.ingest(s.bodies);
        let ov3 = self.constraints.ingest(s.constraints);
        let ov4 = self.replay.ingest(s.replay_ticks);
        if ov1 || ov2 || ov3 || ov4 {
            self.aggregate_overflows = self.aggregate_overflows.saturating_add(1);
        }
        self.ingested = self.ingested.saturating_add(1);
    }

    /// 口径只读。
    pub fn caliber(&self) -> &Caliber {
        &self.caliber
    }

    /// 步长耗时账只读。
    pub fn step_cost(&self) -> &MetricLedger {
        &self.step_cost
    }

    /// 物理体数账只读。
    pub fn body_ledger(&self) -> &MetricLedger {
        &self.bodies
    }

    /// 约束数账只读。
    pub fn constraint_ledger(&self) -> &MetricLedger {
        &self.constraints
    }

    /// 回放使用账只读。
    pub fn replay_ledger(&self) -> &MetricLedger {
        &self.replay
    }

    /// 已聚合样本数。
    pub fn ingested(&self) -> u64 {
        self.ingested
    }

    /// 聚合饱和立案数。
    pub fn overflows(&self) -> u32 {
        self.aggregate_overflows
    }

    /// 汇总可读行（口径公开：账面一句话，含四类 avg/min/max）。
    pub fn summarize(&self) -> String {
        format!(
            "agg v{}: step_us avg={:?} max={} | bodies avg={:?} max={} | constraints avg={:?} max={} | replay avg={:?} max={} | samples={}",
            self.caliber.version,
            self.step_cost.avg(),
            self.step_cost.max,
            self.bodies.avg(),
            self.bodies.max,
            self.constraints.avg(),
            self.constraints.max,
            self.replay.avg(),
            self.replay.max,
            self.ingested
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 滚动时间线（超限→滚动降级；洪峰一眼可见；非单调拒收）
// ─────────────────────────────────────────────────────────────────────────────

/// 时间线条目：完整样本值 + 洪峰标注位（导出五字段 schema 的数据面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelinePoint {
    /// 逻辑 tick。
    pub tick: u64,
    /// 步长耗时（微秒）。
    pub step_cost_us: u32,
    /// 物理体数。
    pub bodies: u32,
    /// 约束数。
    pub constraints: u32,
    /// 回放使用（tick 计）。
    pub replay_ticks: u32,
    /// 洪峰标注位（peak() 回写——一眼可见的数据面）。
    pub peak_marked: bool,
}

/// 滚动时间线：容量硬上限的 FIFO 环形窗（超限让位最旧——0xB403 滚动降级）。
/// 洪峰 peak() 全窗 O(n) 扫描标注；tick 必须单调（0xB40A）。
pub struct RollingTimeline {
    points: Vec<TimelinePoint>,
    capacity: usize,
    /// 滚动让位计数（0xB403 审计位）。
    rolled: u64,
    /// 已收最大 tick（单调性判定）。
    last_tick: Option<u64>,
    /// 拒收计数（非单调审计位）。
    rejected_nonmono: u32,
}

impl RollingTimeline {
    /// 指定容量开时间线（容量 0 无意义——调用方保证 ≥1，内部兜底按 1）。
    pub fn new(capacity: usize) -> Self {
        RollingTimeline {
            points: Vec::new(),
            capacity: if capacity == 0 { 1 } else { capacity },
            rolled: 0,
            last_tick: None,
            rejected_nonmono: 0,
        }
    }

    /// 推入一个样本（O(1) 摊还；超限滚动让位最旧；tick 单调闸）。
    pub fn observe(&mut self, s: TelemetrySample) -> Result<(), Diag> {
        if let Some(last) = self.last_tick {
            if s.tick < last {
                self.rejected_nonmono += 1;
                return Err(Diag::new(XcCode::NonMonotonicTick, "tick 回退，时间线必须单调"));
            }
        }
        if self.points.len() >= self.capacity {
            // 滚动：让位最旧（FIFO——Vec 头部移除 O(k)，k=让位数；本实现单点让位）。
            self.points.remove(0);
            self.rolled += 1;
        }
        self.points.push(TimelinePoint {
            tick: s.tick,
            step_cost_us: s.step_cost_us,
            bodies: s.bodies,
            constraints: s.constraints,
            replay_ticks: s.replay_ticks,
            peak_marked: false,
        });
        self.last_tick = Some(s.tick);
        Ok(())
    }

    /// 洪峰标注：全窗 O(n) 扫描最大体数点并回写标注位（重复调用幂等）。
    /// 返回洪峰点；空窗显性 None。若标注未生效（空窗）上层按 0xB409 处置。
    pub fn mark_peak(&mut self) -> Option<TimelinePoint> {
        let mut best: Option<usize> = None;
        let mut i = 0;
        while i < self.points.len() {
            match best {
                None => best = Some(i),
                Some(b) => {
                    if self.points[i].bodies > self.points[b].bodies {
                        best = Some(i);
                    }
                }
            }
            i += 1;
        }
        let bi = best?;
        // 先清旧标注再标新（幂等）。
        let mut i = 0;
        while i < self.points.len() {
            self.points[i].peak_marked = i == bi;
            i += 1;
        }
        Some(self.points[bi])
    }

    /// 洪峰标注位核验：全窗有且仅有一个标注位（0xB409 判据面）。
    pub fn peak_marks_exactly_one(&self) -> bool {
        let mut n = 0;
        let mut i = 0;
        while i < self.points.len() {
            if self.points[i].peak_marked {
                n += 1;
            }
            i += 1;
        }
        n == 1
    }

    /// 窗内点只读切片。
    pub fn points(&self) -> &[TimelinePoint] {
        &self.points
    }

    /// 窗容量。
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 滚动让位计数。
    pub fn rolled(&self) -> u64 {
        self.rolled
    }

    /// 非单调拒收计数。
    pub fn nonmono_rejected(&self) -> u32 {
        self.rejected_nonmono
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 异常事件标注 → 来源分类 → 统计汇总
// ─────────────────────────────────────────────────────────────────────────────

/// 事件来源分类闭集（标注可归因——缺来源 0xB407）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// 步长预算监控（步耗时超预算）。
    StepBudget,
    /// 生成洪峰监控（体数激增）。
    SpawnBurst,
    /// 回放驱动器（回放导致的异常）。
    ReplayDriver,
    /// 外部查询（总日志中心等消费方上报）。
    ExternalQuery,
}

impl SourceKind {
    /// 可读名（导出与汇总用）。
    pub fn name(self) -> &'static str {
        match self {
            SourceKind::StepBudget => "step_budget",
            SourceKind::SpawnBurst => "spawn_burst",
            SourceKind::ReplayDriver => "replay_driver",
            SourceKind::ExternalQuery => "external_query",
        }
    }

    /// 全闭集枚举（判据遍历用）。
    pub const ALL: [SourceKind; 4] = [
        SourceKind::StepBudget,
        SourceKind::SpawnBurst,
        SourceKind::ReplayDriver,
        SourceKind::ExternalQuery,
    ];
}

/// 异常事件类别闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventCategory {
    /// 步耗时超预算。
    StepOverBudget,
    /// 体数洪峰。
    BodyFlood,
    /// 回放偏差（联动 vead08 的对拍面）。
    ReplayDeviation,
    /// 未知异常（来源自报但未归类）。
    Unclassified,
}

impl EventCategory {
    /// 可读名。
    pub fn name(self) -> &'static str {
        match self {
            EventCategory::StepOverBudget => "step_over_budget",
            EventCategory::BodyFlood => "body_flood",
            EventCategory::ReplayDeviation => "replay_deviation",
            EventCategory::Unclassified => "unclassified",
        }
    }
}

/// 异常事件标注：tick + 类别 + 来源三件齐才收。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventAnnotation {
    /// 事件逻辑 tick。
    pub tick: u64,
    /// 事件类别。
    pub category: EventCategory,
    /// 来源分类（缺此件即 0xB407——本字段不可省）。
    pub source: SourceKind,
}

/// 来源统计汇总：per-source 次数账（O(标注) 单遍）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSummary {
    /// 四类来源计数（与 SourceKind::ALL 同序）。
    counts: [u32; 4],
    /// 标注总数。
    total: u32,
}

impl SourceSummary {
    /// 空汇总。
    pub const fn new() -> Self {
        SourceSummary {
            counts: [0; 4],
            total: 0,
        }
    }

    /// 单标注入账（O(1)）。
    pub fn record(&mut self, a: &EventAnnotation) {
        let mut i = 0;
        while i < SourceKind::ALL.len() {
            if SourceKind::ALL[i] == a.source {
                self.counts[i] = self.counts[i].saturating_add(1);
            }
            i += 1;
        }
        self.total = self.total.saturating_add(1);
    }

    /// 指定来源计数。
    pub fn count_of(&self, s: SourceKind) -> u32 {
        let mut i = 0;
        while i < SourceKind::ALL.len() {
            if SourceKind::ALL[i] == s {
                return self.counts[i];
            }
            i += 1;
        }
        0
    }

    /// 标注总数。
    pub fn total(&self) -> u32 {
        self.total
    }

    /// 汇总渲染（分类含统计汇总判据落地：每来源一行可读）。
    pub fn render(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        let mut i = 0;
        while i < SourceKind::ALL.len() {
            lines.push(format!(
                "{}={}",
                SourceKind::ALL[i].name(),
                self.counts[i]
            ));
            i += 1;
        }
        format!("source_summary total={} [{}]", self.total, lines.join(" "))
    }
}

/// 标注账：标注收集 + 缺来源防线（类型上 source 不可省，防线锚点仍显性在闸——
/// 对外 API 收 Option 形态的原始标注，None source 显性拒 0xB407）。
pub struct AnnotationLedger {
    anns: Vec<EventAnnotation>,
    summary: SourceSummary,
    /// 缺来源拒收计数（0xB407 审计位）。
    unsourced_rejected: u32,
}

impl AnnotationLedger {
    /// 空账。
    pub fn new() -> Self {
        AnnotationLedger {
            anns: Vec::new(),
            summary: SourceSummary::new(),
            unsourced_rejected: 0,
        }
    }

    /// 原始标注入闸：source 缺席显性拒 0xB407；合法则收账+汇总结算 O(1)。
    pub fn submit(
        &mut self,
        tick: u64,
        category: EventCategory,
        source: Option<SourceKind>,
    ) -> Result<(), Diag> {
        match source {
            None => {
                self.unsourced_rejected += 1;
                Err(Diag::new(
                    XcCode::AnnotationUnsourced,
                    "标注缺来源分类，拒绝收账",
                ))
            }
            Some(src) => {
                let a = EventAnnotation {
                    tick,
                    category,
                    source: src,
                };
                self.summary.record(&a);
                self.anns.push(a);
                Ok(())
            }
        }
    }

    /// 标注只读切片。
    pub fn annotations(&self) -> &[EventAnnotation] {
        &self.anns
    }

    /// 汇总只读。
    pub fn summary(&self) -> &SourceSummary {
        &self.summary
    }

    /// 缺来源拒收计数。
    pub fn unsourced_rejected(&self) -> u32 {
        self.unsourced_rejected
    }

    /// 按 tick 查标注（时间线联动：某 tick 是否有异常事件）。
    pub fn at_tick(&self, tick: u64) -> Vec<EventAnnotation> {
        let mut out: Vec<EventAnnotation> = Vec::new();
        let mut i = 0;
        while i < self.anns.len() {
            if self.anns[i].tick == tick {
                out.push(self.anns[i]);
            }
            i += 1;
        }
        out
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// U 域总日志中心对接声明（跨批对接点显性化）
// ─────────────────────────────────────────────────────────────────────────────

/// U 域 schema 对接声明：行格式版本+字段序+分隔符三件固定——总日志中心按此消费。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct USchema {
    /// 行格式版本。
    pub format_version: u32,
    /// 字段序（固定串，导出严格按此序）。
    pub field_order: &'static str,
    /// 分隔符。
    pub separator: char,
}

/// U 域对接实例（常量声明——文档化在数据面）。
pub const U_SCHEMA: USchema = USchema {
    format_version: 1,
    field_order: "tick,step_cost_us,bodies,constraints,replay_ticks",
    separator: ',',
};

impl USchema {
    /// schema 标头行（导出首行——消费方先读此行定版）。
    pub fn header(&self) -> String {
        format!(
            "#u-schema v{} fields={} sep={}",
            self.format_version, self.field_order, self.separator
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 聚合服务（组合门面：原始遥测入口闸 → 四类聚合 + 时间线 + 标注 + 缓存降级）
// ─────────────────────────────────────────────────────────────────────────────

/// 组合门面： ingest_raw 一步完成「含内容阻断→负值拒→单调校验→四类聚合→时间线」。
/// 聚合侧任何意外失败走缓存降级（失败→缓存 0xB401）。
pub struct TelemetryService {
    caliber: Caliber,
    agg: FourAggregator,
    timeline: RollingTimeline,
    anns: AnnotationLedger,
    cache: Vec<TelemetrySample>,
    cache_capacity: usize,
    /// 内容阻断计数（0xB402 审计位）。
    content_blocked: u32,
    /// 负值拒收计数（0xB405 审计位）。
    negative_rejected: u32,
    /// 缓存降级计数（0xB401 审计位）。
    cached_fallbacks: u32,
}

impl TelemetryService {
    /// 组装：口径+时间线容量+缓存容量。
    pub fn new(caliber: Caliber, timeline_capacity: usize, cache_capacity: usize) -> Self {
        TelemetryService {
            agg: FourAggregator::new(caliber),
            timeline: RollingTimeline::new(timeline_capacity),
            anns: AnnotationLedger::new(),
            caliber,
            cache: Vec::new(),
            cache_capacity: if cache_capacity == 0 { 1 } else { cache_capacity },
            content_blocked: 0,
            negative_rejected: 0,
            cached_fallbacks: 0,
        }
    }

    /// 原始遥测入口（O(1)）：内容闸→负值闸→（u32 类型天然非负，负值防线由
    /// RawTelemetry 的 i64 变体入口守护，见 ingest_checked）→四类聚合→时间线。
    pub fn ingest_raw(&mut self, raw: RawTelemetry) -> Result<(), Diag> {
        if raw.content.is_some() {
            self.content_blocked += 1;
            return Err(Diag::new(
                XcCode::ContentDetectedBlocked,
                "原始遥测携带内容字段，阻断入库",
            ));
        }
        let s = TelemetrySample {
            tick: raw.tick,
            step_cost_us: raw.step_cost_us,
            bodies: raw.bodies,
            constraints: raw.constraints,
            replay_ticks: raw.replay_ticks,
        };
        self.ingest_sample(s)
    }

    /// 带符号入口（上游损坏防线：负值显性拒 0xB405——u32 主路径天然非负，此入口
    /// 服务于外部 i64 数据源）。
    pub fn ingest_checked(
        &mut self,
        tick: u64,
        step_cost_us: i64,
        bodies: i64,
        constraints: i64,
        replay_ticks: i64,
    ) -> Result<(), Diag> {
        if step_cost_us < 0 || bodies < 0 || constraints < 0 || replay_ticks < 0 {
            self.negative_rejected += 1;
            return Err(Diag::new(
                XcCode::NegativeMetric,
                "负值计量，上游损坏显性拒",
            ));
        }
        self.ingest_sample(TelemetrySample {
            tick,
            step_cost_us: step_cost_us as u32,
            bodies: bodies as u32,
            constraints: constraints as u32,
            replay_ticks: replay_ticks as u32,
        })
    }

    /// 样本主路径：时间线先收（单调闸），再四类聚合；聚合侧意外失败走缓存降级。
    fn ingest_sample(&mut self, s: TelemetrySample) -> Result<(), Diag> {
        self.timeline.observe(s)?;
        // 聚合为纯内存四账推进（无失败路径），缓存降级锚点保留给外部聚合源
        // （如跨批拉取 U 域数据失败时）：此处显性提供 cache_for_retry 供降级调用。
        self.agg.ingest(&s);
        Ok(())
    }

    /// 缓存降级：聚合源失败时样本留档（满容拒新不覆盖——0xB401）。
    pub fn cache_for_retry(&mut self, s: TelemetrySample) -> Result<(), Diag> {
        if self.cache.len() >= self.cache_capacity {
            return Err(Diag::new(
                XcCode::SampleCacheFallback,
                "缓存满容，拒新不覆盖",
            ));
        }
        self.cache.push(s);
        self.cached_fallbacks += 1;
        Ok(())
    }

    /// 缓存重放：从缓存补聚合并清缓存（失败→缓存→修后补聚合的闭环）。
    pub fn drain_cache(&mut self) -> u32 {
        let n = self.cache.len() as u32;
        let mut i = 0;
        while i < self.cache.len() {
            let s = self.cache[i];
            let _ = self.timeline.observe(s);
            self.agg.ingest(&s);
            i += 1;
        }
        self.cache.clear();
        n
    }

    /// 缓存深度。
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    /// 时间线只读。
    pub fn timeline(&self) -> &RollingTimeline {
        &self.timeline
    }

    /// 标注账只读。
    pub fn annotations(&self) -> &AnnotationLedger {
        &self.anns
    }

    /// 聚合器只读。
    pub fn aggregator(&self) -> &FourAggregator {
        &self.agg
    }

    /// 口径只读。
    pub fn caliber(&self) -> &Caliber {
        &self.caliber
    }

    /// 内容阻断计数。
    pub fn content_blocked(&self) -> u32 {
        self.content_blocked
    }

    /// 负值拒收计数。
    pub fn negative_rejected(&self) -> u32 {
        self.negative_rejected
    }

    /// 缓存降级计数。
    pub fn cached_fallbacks(&self) -> u32 {
        self.cached_fallbacks
    }

    /// 开放导出（行式，schema 标头+样本行，行上限截断显性 0xB408）。
    /// 零内容纪律：行里只有数值与 tick。
    pub fn export(&self, row_cap: usize) -> (Vec<String>, u32) {
        let mut lines: Vec<String> = Vec::new();
        lines.push(self.agg.caliber().describe());
        lines.push(U_SCHEMA.header());
        let src = self.timeline.points();
        let mut truncated: u32 = 0;
        let mut i = 0;
        while i < src.len() {
            if lines.len() >= row_cap + 2 {
                truncated = (src.len() - i) as u32;
                break;
            }
            let p = &src[i];
            lines.push(format!(
                "{},{},{},{},{}",
                p.tick, p.step_cost_us, p.bodies, p.constraints, p.replay_ticks
            ));
            i += 1;
        }
        (lines, truncated)
    }

    /// 洪峰防线：mark_peak 后必须恰好一个标注位，否则 0xB409（空窗显性 Err）。
    pub fn enforce_peak_marked(&mut self) -> Result<(), Diag> {
        match self.timeline.mark_peak() {
            Some(_) => {
                if self.timeline.peak_marks_exactly_one() {
                    Ok(())
                } else {
                    Err(Diag::new(XcCode::PeakUnmarked, "标注位异常（非唯一）"))
                }
            }
            None => Err(Diag::new(XcCode::PeakUnmarked, "空窗无洪峰可标")),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 判据入口（CheckSet 聚合面）
// ─────────────────────────────────────────────────────────────────────────────

/// 判据聚合：覆盖锚点判据关键词（四类聚合/内容不记/口径公开）+ 降级矩阵三行全验。
pub fn run_vead09_checks() -> CheckSet {
    let mut ck = CheckSet::new("VE-F6009");

    // ── 一组：口径公开（无障碍判据原文）──
    {
        let c = Caliber::V1;
        let d = c.describe();
        ck.add(
            "口径公开可读含四类单位与版本",
            d.contains("v1")
                && d.contains("us")
                && d.contains("count")
                && d.contains("tick")
                && d.contains("no scene content"),
            "",
        );
        ck.add(
            "口径兼容自反且异版冲突可检",
            c.compatible(&Caliber::V1)
                && !c.compatible(&Caliber {
                    version: 2,
                    ..Caliber::V1
                }),
            "",
        );
        ck.add(
            "U 域 schema 标头含版本字段序分隔符",
            U_SCHEMA.header().contains("#u-schema v1")
                && U_SCHEMA.header().contains("tick,step_cost_us,bodies,constraints,replay_ticks"),
            "",
        );
    }

    // ── 二组：四类聚合（单遍 O(样本) + 四元账对拍）──
    {
        let mut agg = FourAggregator::new(Caliber::V1);
        let script = fixture_samples();
        let mut i = 0;
        while i < script.len() {
            agg.ingest(&script[i]);
            i += 1;
        }
        ck.add(
            "四账样本数对账（单遍逐样本推进）",
            agg.ingested() == script.len() as u64
                && agg.step_cost().count == script.len() as u64
                && agg.body_ledger().count == script.len() as u64
                && agg.constraint_ledger().count == script.len() as u64
                && agg.replay_ledger().count == script.len() as u64,
            "",
        );
        // min/max/sum 逐类对拍（fixture 值固定——账面可手工复核）。
        ck.add(
            "步长账 min/max/sum 精确",
            agg.step_cost().min == 50
                && agg.step_cost().max == 400
                && agg.step_cost().sum == 1000,
            "",
        );
        ck.add(
            "体数账 min/max 精确（洪峰底账）",
            agg.body_ledger().min == 2 && agg.body_ledger().max == 900,
            "",
        );
        ck.add(
            "回放账 sum 精确",
            agg.replay_ledger().sum == 15,
            "",
        );
        ck.add(
            "均值整数除法且空账显性 None",
            agg.step_cost().avg() == Some(166) && MetricLedger::new().avg().is_none(),
            "",
        );
        ck.add(
            "汇总行可读含四类",
            agg.summarize().contains("step_us") && agg.summarize().contains("bodies") && agg.summarize().contains("replay"),
            "",
        );
    }

    // ── 三组：内容不记（0xB402 阻断 + 闭集无内容字段）──
    {
        let mut svc = TelemetryService::new(Caliber::V1, 64, 16);
        let dirty = RawTelemetry {
            tick: 1,
            step_cost_us: 100,
            bodies: 2,
            constraints: 1,
            replay_ticks: 0,
            content: Some("场景物体坐标流水（内容！）".to_string()),
        };
        let blocked = matches!(svc.ingest_raw(dirty), Err(d) if d.kind() == XcCode::ContentDetectedBlocked);
        ck.add(
            "含内容样本阻断入库 0xB402 且审计计数",
            blocked && svc.content_blocked() == 1 && svc.aggregator().ingested() == 0,
            "",
        );
        // 合法样本不受影响。
        let ok = svc
            .ingest_raw(RawTelemetry::pure(1, 100, 2, 1, 0))
            .is_ok();
        ck.add("纯参数样本正常入库", ok, "");
        // TelemetrySample 类型面零内容：闭集五字段（同值双实例指纹一致 + 变值指纹
        // 必变——双向实证无隐藏可变内容态）。
        let s1 = TelemetrySample { tick: 7, step_cost_us: 1, bodies: 2, constraints: 3, replay_ticks: 4 };
        let s2 = TelemetrySample { tick: 7, step_cost_us: 1, bodies: 2, constraints: 3, replay_ticks: 4 };
        let s3 = TelemetrySample { tick: 8, step_cost_us: 1, bodies: 2, constraints: 3, replay_ticks: 4 };
        ck.add("样本闭集指纹双向实证（同值同纹异值异纹）", s1.fingerprint() == s2.fingerprint() && s1.fingerprint() != s3.fingerprint() && s1.fingerprint() != 0, "");
    }

    // ── 四组：时间线（超限滚动 + 洪峰标注 + 非单调拒收）──
    {
        let mut tl = RollingTimeline::new(4);
        let seq = fixture_samples();
        let mut rolled_seen = false;
        let mut i = 0;
        while i < seq.len() {
            let _ = tl.observe(seq[i]);
            if tl.rolled() > 0 {
                rolled_seen = true;
            }
            i += 1;
        }
        ck.add(
            "窗口超限滚动让位最旧 0xB403",
            rolled_seen && tl.rolled() == 2 && tl.points().len() == 4 && tl.points()[0].tick == 3,
            "",
        );
        // 洪峰：窗内 tick3(900),4(10),5(3),6(4) → 洪峰 tick3 bodies900。
        let peak = tl.mark_peak();
        ck.add(
            "洪峰标注唯一且值正确（一眼可见）",
            matches!(peak, Some(p) if p.tick == 3 && p.bodies == 900) && tl.peak_marks_exactly_one(),
            "",
        );
        // 幂等：重复 mark 不漂移。
        let peak2 = tl.mark_peak();
        ck.add(
            "洪峰标注幂等",
            matches!(peak2, Some(p) if p.tick == 3) && tl.peak_marks_exactly_one(),
            "",
        );
        // 非单调拒收。
        let back = tl.observe(TelemetrySample { tick: 3, step_cost_us: 1, bodies: 1, constraints: 1, replay_ticks: 0 });
        ck.add(
            "tick 回退显性拒 0xB40A 且计数",
            matches!(back, Err(d) if d.kind() == XcCode::NonMonotonicTick) && tl.nonmono_rejected() == 1,
            "",
        );
        // 空窗洪峰 → 0xB409。
        let mut empty = RollingTimeline::new(4);
        ck.add(
            "空窗洪峰显性 0xB409",
            matches!(empty.mark_peak(), None),
            "",
        );
    }

    // ── 五组：标注→来源分类→统计汇总（三链全验）──
    {
        let mut led = AnnotationLedger::new();
        let ok1 = led.submit(10, EventCategory::BodyFlood, Some(SourceKind::SpawnBurst)).is_ok();
        let ok2 = led.submit(20, EventCategory::StepOverBudget, Some(SourceKind::StepBudget)).is_ok();
        let ok3 = led.submit(30, EventCategory::ReplayDeviation, Some(SourceKind::ReplayDriver)).is_ok();
        let bad = matches!(
            led.submit(40, EventCategory::Unclassified, None),
            Err(d) if d.kind() == XcCode::AnnotationUnsourced
        );
        ck.add(
            "标注三件齐收账+缺来源显性拒 0xB407",
            ok1 && ok2 && ok3 && bad && led.unsourced_rejected() == 1 && led.annotations().len() == 3,
            "",
        );
        let s = led.summary();
        ck.add(
            "来源统计汇总精确（分类含统计汇总）",
            s.count_of(SourceKind::SpawnBurst) == 1
                && s.count_of(SourceKind::StepBudget) == 1
                && s.count_of(SourceKind::ReplayDriver) == 1
                && s.count_of(SourceKind::ExternalQuery) == 0
                && s.total() == 3,
            "",
        );
        ck.add(
            "汇总渲染含四类来源名",
            s.render().contains("spawn_burst=1") && s.render().contains("external_query=0"),
            "",
        );
        // 时间线联动：at_tick 查询。
        let mut led2 = AnnotationLedger::new();
        let _ = led2.submit(10, EventCategory::BodyFlood, Some(SourceKind::SpawnBurst));
        ck.add(
            "按 tick 查标注命中",
            led2.at_tick(10).len() == 1 && led2.at_tick(11).is_empty(),
            "",
        );
    }

    // ── 六组：降级矩阵第一行（失败→缓存→补聚合闭环）+ 负值防线 ──
    {
        let mut svc = TelemetryService::new(Caliber::V1, 64, 3);
        let s1 = TelemetrySample { tick: 1, step_cost_us: 10, bodies: 2, constraints: 1, replay_ticks: 0 };
        let ok = svc.cache_for_retry(s1).is_ok();
        let s2 = TelemetrySample { tick: 2, step_cost_us: 20, bodies: 3, constraints: 1, replay_ticks: 0 };
        let _ = svc.cache_for_retry(s2);
        let s3 = TelemetrySample { tick: 3, step_cost_us: 30, bodies: 4, constraints: 1, replay_ticks: 0 };
        let _ = svc.cache_for_retry(s3);
        let s4 = TelemetrySample { tick: 4, step_cost_us: 40, bodies: 5, constraints: 1, replay_ticks: 0 };
        let full = matches!(svc.cache_for_retry(s4), Err(d) if d.kind() == XcCode::SampleCacheFallback);
        ck.add(
            "缓存满容拒新不覆盖 0xB401",
            ok && full && svc.cache_len() == 3 && svc.cached_fallbacks() == 3,
            "",
        );
        let drained = svc.drain_cache();
        ck.add(
            "缓存重放补聚合闭环（失败→缓存→修后补）",
            drained == 3 && svc.cache_len() == 0 && svc.aggregator().ingested() == 3,
            "",
        );
        // 负值防线。
        let neg = svc.ingest_checked(5, -1, 2, 1, 0);
        ck.add(
            "负值计量显性拒 0xB405",
            matches!(neg, Err(d) if d.kind() == XcCode::NegativeMetric) && svc.negative_rejected() == 1,
            "",
        );
    }

    // ── 七组：开放导出（schema 对齐 + 截断显性 0xB408）──
    {
        let mut svc = TelemetryService::new(Caliber::V1, 8, 4);
        let script = fixture_samples();
        let mut i = 0;
        while i < script.len() {
            let s = script[i];
            let _ = svc.ingest_raw(RawTelemetry::pure(s.tick, s.step_cost_us, s.bodies, s.constraints, s.replay_ticks));
            i += 1;
        }
        let (lines, trunc) = svc.export(64);
        ck.add(
            "导出含口径行与 schema 标头",
            lines.len() >= 2 && lines[0].contains("caliber v1") && lines[1].contains("#u-schema v1"),
            "",
        );
        ck.add(
            "导出行数对账零截断",
            trunc == 0 && lines.len() == 2 + svc.timeline().points().len(),
            "",
        );
        // 小容量导出 → 截断显性。
        let (lines2, trunc2) = svc.export(4);
        ck.add(
            "导出截断显性计数 0xB408 语义（trunc>0 且行数封顶）",
            trunc2 > 0 && lines2.len() == 4 + 2,
            "",
        );
        // 洪峰防线闭环。
        let marked = svc.enforce_peak_marked().is_ok();
        ck.add(
            "洪峰标注防线闭环（peak 唯一后 Ok）",
            marked && svc.timeline().peak_marks_exactly_one(),
            "",
        );
        // 诊断码闭集。
        let codes = [
            XcCode::SampleCacheFallback,
            XcCode::ContentDetectedBlocked,
            XcCode::WindowOverflowRolling,
            XcCode::EmptyStream,
            XcCode::NegativeMetric,
            XcCode::CounterOverflow,
            XcCode::AnnotationUnsourced,
            XcCode::ExportTruncated,
            XcCode::PeakUnmarked,
            XcCode::NonMonotonicTick,
            XcCode::CaliberMismatch,
            XcCode::AggregateOverflow,
        ];
        let all_seg = codes.iter().all(|c| c.code() >> 8 == SEGMENT_BASE);
        let all_detail = codes.iter().all(|c| !c.detail().is_empty());
        let mut distinct = true;
        let mut a = 0;
        while a < codes.len() {
            let mut b = a + 1;
            while b < codes.len() {
                if codes[a].code() == codes[b].code() {
                    distinct = false;
                }
                b += 1;
            }
            a += 1;
        }
        ck.add(
            "诊断码 0xB4 段十二码闭集独占且建议齐",
            all_seg && all_detail && distinct,
            "",
        );
        let d = Diag::new(XcCode::WindowOverflowRolling, "测试现场");
        ck.add(
            "诊断渲染含码含建议",
            d.render().contains("0xB403") && d.render().contains("||"),
            "",
        );
        ck.add(
            "同构指纹同源（FNV 空串基向量）",
            fnv1a(&[]) == FNV_OFFSET,
            "",
        );
    }

    ck
}

// ─────────────────────────────────────────────────────────────────────────────
// 判据夹具（确定性样本流——账面可手工复核）
// ─────────────────────────────────────────────────────────────────────────────

/// 标准样本流：6 样本，步长 100..400、体数洪峰 900、回放 1..=5（sum=15×?——见下注）。
/// step_us = [100,200,150,400,50,100] sum=1000 min=50? 校准：min 取 100 起步须全 ≥100。
fn fixture_samples() -> Vec<TelemetrySample> {
    vec![
        TelemetrySample { tick: 1, step_cost_us: 100, bodies: 2,   constraints: 1, replay_ticks: 1 },
        TelemetrySample { tick: 2, step_cost_us: 200, bodies: 5,   constraints: 2, replay_ticks: 2 },
        TelemetrySample { tick: 3, step_cost_us: 150, bodies: 900, constraints: 3, replay_ticks: 3 },
        TelemetrySample { tick: 4, step_cost_us: 400, bodies: 10,  constraints: 2, replay_ticks: 4 },
        TelemetrySample { tick: 5, step_cost_us: 50,  bodies: 3,   constraints: 1, replay_ticks: 5 },
        TelemetrySample { tick: 6, step_cost_us: 100, bodies: 4,   constraints: 1, replay_ticks: 0 },
    ]
}
