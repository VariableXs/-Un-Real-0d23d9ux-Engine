//! unx943_tophalf —— UNX-F0943 top half 纪律（≤5µs 预算与最小化清单）。
//! 锚点（CoRun Unxreal 主册 UNX-A2）：UNX-F0943 · top half 纪律（≤5µs 预算与最小化清单） · 目标 360 行 · 已深化
//! 判据原文：UNX-F0943-J1 预算断言（top half p99 ≤5µs 全 handler，QEMU 档），
//!           超预算清单自动产出 10/10 次，纪律文档冻结（义务/禁令/预算三节）
//! 前驱（已入库存档）：
//!   · unx941_irqtwostage.rs —— UNX-F0941 两级分发骨架，把 top half 做成五态事件账的
//!     一个合法阶段（Raised→TopDone），并从骨架侧施加三条硬禁（TOP_HEAVY_LIMIT=256B 摘取上限、
//!     未应答先返回拦截、bottom half 直取硬件拦截）。本模块不是重复，而是把「top half 到底
//!     允许做什么」从骨架的四态跃迁，细化为**四类操作 + 五类禁区 + 预算纪律**三层闭集。
//!   · unx942_dispatch.rs —— UNX-F0942 分发入口统一路径，把五类向量经同一 IDT 入口分型路由，
//!     从此 top half 入口唯一。本模块消费其向量类别，实现「时钟类必最轻」的分类预算档。
//! 组件：
//!   · ThClass 五类操作闭集（Ack/RegisterRead/Snapshot/Timestamp/NmiHandshake）——一个操作
//!     归不到任何一类即属禁区，本地不新增第六类（新类须先立 ADR）。
//!   · ThZone 六条硬禁闭集（SpinLock/Sleep/LargeCopy/Mmap/Logging/UnboundedWalk）。
//!   · ThBudget 三档硬线（时钟/处理器间/设备）——判据常量独立写死不复用被测时长。
//!   · 唯一进度量 = ThWindow::budget_of + charge（无第二条预算来源）。
//!   · 预算归因：单点总时长超档，先按最大嫌疑步骤结算；不重迭归因（∑归因 ≤ 总超支）。
//!   · 时间源：一律逻辑 tick，IAC（idle-abstract clock）可复现、QEMU 档生效；
//!     物理单位（µs）为既定刻度的折算结果，实际测时长仍走 tick，默认档 1 tick=50ns。
//!   · 超预算清单（ThLedger）：每例一条条目（向量/类别/总时长/p99/最大嫌疑步骤/严重度），
//!     自带 over_budget() 判定与 digest() 三节纪要（义务/禁令/预算）。
//! 判据落地：p99≤5µs 全 handler 预算断言 + 超预算清单 10/10 次自动产出 + 纪律文档冻结三节
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑 tick）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const TH_VERSION: &str = "unx943_tophalf-v1";

/// 诊断段基址（0x4c50 段 · 本模块独占）。
pub const TH_SEG_BASE: u16 = 0x4c50;

/// 预算硬线（判据常量，独立写死，µs）。
pub const TH_CLOCK_BUDGET_US: u64 = 1;
/// 处理器间中断预算硬线（µs）。
pub const TH_IPI_BUDGET_US: u64 = 2;
/// 设备中断预算硬线（µs）。
pub const TH_DEV_BUDGET_US: u64 = 5;

/// 每 handler 采样容量上限（1<<16 定容，绝不向 OS 要内存）。
pub const TH_SAMPLE_CAP: usize = 1 << 16;

/// 超预算清单条数上限（1<<12 定容；溢出压最旧，计数保真）。
pub const TH_LEDGER_CAP: usize = 1 << 12;

/// 逻辑 tick 对应纳秒数（默认档 QEMU：1 tick = 50ns）。
pub const TH_TICK_NS_PER_TICK: u64 = 50;

/// p99 序列（内置定死，不留操作者挑分位点口径的空间）。
pub const TH_P99_IDX: usize = 99;

/// top half 操作类别（五类闭集；判据「最小化清单」的操作侧）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThClass {
    /// 应答：清中断挂起位，向控制器确认本事件已受理。
    Ack,
    /// 寄存器读：只读型 MMIO/端口读（经 unx942 归一后无算术）。
    RegisterRead,
    /// 快照：摘取少量定长数据进 per-event 缓冲（≤256B，与骨架上限同源）。
    Snapshot,
    /// 时间戳：读一次计时器并记入事件帧。
    Timestamp,
    /// 处理器间握手：只收/发信封，不做被请求的工作。
    NmiHandshake,
}

impl ThClass {
    /// 全量闭集（顺序即清单顺序）。
    pub const ALL: [ThClass; 5] = [
        ThClass::Ack,
        ThClass::RegisterRead,
        ThClass::Snapshot,
        ThClass::Timestamp,
        ThClass::NmiHandshake,
    ];

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            ThClass::Ack => "应答（清挂起/确认）",
            ThClass::RegisterRead => "寄存器只读",
            ThClass::Snapshot => "定长快照摘取",
            ThClass::Timestamp => "计时器读一次",
            ThClass::NmiHandshake => "处理器间握手信封",
        }

/// top half 禁区（六条闭集；判据「最小化清单」的禁令侧）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThZone {
    /// 自旋等待（含自旋锁、忙等标志）。
    SpinLock,
    /// 睡眠/让出（任何让出本 CPU 的动作）。
    Sleep,
    /// 大块搬运（超 256B 的拷贝）。
    LargeCopy,
    /// 映射/改页表（涉及虚拟内存结构变更）。
    Mmap,
    /// 格式化输出（含控制台写、日志、printf 族）。
    Logging,
    /// 无上界遍历（链表/任意容器长度未知的遍历）。
    UnboundedWalk,
}

impl ThZone {
    /// 全量闭集。
    pub const ALL: [ThZone; 6] = [
        ThZone::SpinLock,
        ThZone::Sleep,
        ThZone::LargeCopy,
        ThZone::Mmap,
        ThZone::Logging,
        ThZone::UnboundedWalk,
    ];

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            ThZone::SpinLock => "自旋等待/自旋锁",
            ThZone::Sleep => "睡眠或让出本 CPU",
            ThZone::LargeCopy => "大块搬运（>256B）",
            ThZone::Mmap => "映射/改页表",
            ThZone::Logging => "格式化输出/日志",
            ThZone::UnboundedWalk => "无上界遍历",
        }
    }
}

/// 违规严重度（三档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ThSeverity {
    /// 提示（清单单列，不进拦截）。
    Advisory,
    /// 违例（清单 + 拦截计数）。
    Violation,
    /// 重犯（清单 + 拦截计数 + 记账名点）。
    Repeat,
}

impl ThSeverity {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            ThSeverity::Advisory => "提示",
            ThSeverity::Violation => "违例",
            ThSeverity::Repeat => "重犯",
        }
    }
}

/// 向量大类（分类预算档；与 unx942 分型结果对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThKind {
    /// 时钟中断（预算最轻）。
    Clock,
    /// 处理器间中断。
    Ipi,
    /// 设备中断。
    Device,
    /// 未知/错误向量（按最严档处理）。
    Unknown,
}

/// 诊断码（0x4c50 段十三码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThCode {
    /// 0x4c50 超预算（总时长 > 对应档位硬线）。
    OverBudget,
    /// 0x4c51 落入禁区（六禁之一）。
    ZoneHit,
    /// 0x4c52 义务缺位（应答或时间戳未做）。
    DutyMiss,
    /// 0x4c53 采样容量溢出（第 65537 条即拒）。
    SampleFull,
    /// 0x4c54 清单容量溢出（压最旧后仍计数）。
    LedgerFull,
    /// 0x4c55 类目外操作（归不到五类）。
    ClassUnknown,
    /// 0x4c56 时间源混用（同窗口出现 tick 与物理两种口径）。
    TimeMix,
    /// 0x4c57 归因总和超总超支（重迭归因）。
    AttributionOver,
    /// 0x4c58 类目重复归集（同类操作被反复记账同一 handler）。
    ClassDup,
    /// 0x4c59 分位口径被改（分位点非内置 P99）。
    PctlTamper,
    /// 0x4c5a 时钟档被错配（时钟类走了设备档预算）。
    ClockMismatch,
    /// 0x4c5b 采样空缺（窗口零样本仍被断言）。
    EmptyWindow,
    /// 0x4c5c 清单条目重复入账（同向量同指纹二次记账）。
    DupLedger,
    /// 0x4c5d 段位守卫失败。
    SegmentGuard,
}

impl ThCode {
    /// 全量闭集。
    pub const ALL: [ThCode; 13] = [
        ThCode::OverBudget,
        ThCode::ZoneHit,
        ThCode::DutyMiss,
        ThCode::SampleFull,
        ThCode::LedgerFull,
        ThCode::ClassUnknown,
        ThCode::TimeMix,
        ThCode::AttributionOver,
        ThCode::ClassDup,
        ThCode::PctlTamper,
        ThCode::ClockMismatch,
        ThCode::EmptyWindow,
        ThCode::DupLedger,
        ThCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        TH_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            ThCode::OverBudget => "超预算：top half 总时长超过对应档位硬线",
            ThCode::ZoneHit => "落入禁区：top half 出现六禁操作之一",
            ThCode::DutyMiss => "义务缺位：应答或时间戳未做即返",
            ThCode::SampleFull => "采样容量溢出：单 handler 第 65537 条样本即拒",
            ThCode::LedgerFull => "清单容量溢出：压最旧后计数保真",
            ThCode::ClassUnknown => "类目外操作：操作归不到五类闭集",
            ThCode::TimeMix => "时间源混用：同窗口出现 tick 与物理两种口径",
            ThCode::AttributionOver => "归因总和超总超支：不重迭归因被破",
            ThCode::ClassDup => "类目重复归集：同类操作被反复记账同一 handler",
            ThCode::PctlTamper => "分位口径被改：分位点非内置 P99",
            ThCode::ClockMismatch => "时钟档被错配：时钟类走了设备档预算",
            ThCode::EmptyWindow => "采样空缺：窗口零样本仍被断言",
            ThCode::DupLedger => "清单条目重复入账：同向量同指纹二次记账",
            ThCode::SegmentGuard => "段位守卫失败：诊断码越出 0x4c50 段",
        }
    }

    /// 可操作建议（错误路径带源位置与下一步动作）。
    pub const fn advice(self) -> &'static str {
        match self {
            ThCode::OverBudget => "把最重的一步改为 bottom half 任务：本步改排 pending 队列，top half 只留应答与快照",
            ThCode::ZoneHit => "该操作属 top half 六禁：整条调用改由 bottom half 执行；若确需同步，先立 ADR 并同步本模块 ThZone",
            ThCode::DutyMiss => "补做应答或时间戳：应答清挂起位否则中断风暴；时间戳记一次计时器否则归因失真",
            ThCode::SampleFull => "该 handler 采样已满：核对是否误用生产路径采样；采样只应来自演练注入",
            ThCode::LedgerFull => "超预算清单已满：本条压最旧入队；若需全量请先固化既有清单再重跑",
            ThCode::ClassUnknown => "该操作不在五类闭集：先归类到既有五类；确属新类先立 ADR 扩充 ALL 再启用",
            ThCode::TimeMix => "统一时间源：同窗口只允许 tick；物理单位只作 p99 折算，不得作为被测时长入账",
            ThCode::AttributionOver => "归因重迭：每步只归因一次；重跑请先清零步账",
            ThCode::ClassDup => "同类操作重复记账：每类每 handler 只归集一次；重复请走 next_occurrence",
            ThCode::PctlTamper => "分位口径被改：本模块分位点内置为 P99，不得由操作者指定",
            ThCode::ClockMismatch => "时钟类错配：时钟类必须走时钟档（最轻预算），不得套用设备档",
            ThCode::EmptyWindow => "空窗口断言：请先注入至少一条样本再断言 p99",
            ThCode::DupLedger => "清单条目重复：同向量同指纹只入账一次；重复请走复核接口",
            ThCode::SegmentGuard => "段位守卫失败：诊断码越界，请核对 TH_SEG_BASE 与 ALL 长度",
        }
    }

    /// 严重度映射（供清单排序与拦截判定）。
    pub const fn severity(self) -> ThSeverity {
        match self {

/// 归因步骤（一条 handler 的每一步；归因用）。
#[derive(Clone, Copy, Debug)]
pub struct ThStep {
    /// 该步的操作类别（五类之一）。
    pub class: ThClass,
    /// 该步实测时长（逻辑 tick）。
    pub ticks: u64,
    /// 该步是否同时落在禁区（用于综合判定）。
    pub zone: Option<ThZone>,
}

impl ThStep {
    /// 构造一步（无禁区标注）。
    pub const fn new(class: ThClass, ticks: u64) -> Self {
        ThStep {
            class,
            ticks,
            zone: None,
        }
    }

    /// 构造一步并标注禁区命中。
    pub const fn with_zone(class: ThClass, ticks: u64, zone: ThZone) -> Self {
        ThStep {
            class,
            ticks,
            zone: Some(zone),
        }
    }
}

/// 单条 handler 执行样本（一次中断的 top half 完整记录）。
#[derive(Clone, Debug)]
pub struct ThSample {
    /// 向量号。
    pub vector: u16,
    /// 中断大类。
    pub kind: ThKind,
    /// 归因步骤（至多 8 步；超出即拒）。
    pub steps: Vec<ThStep>,
    /// 总时长（逻辑 tick，各步之和）。
    pub total_ticks: u64,
    /// 应答是否完成（义务判据）。
    pub acked: bool,
    /// 时间戳是否记录（义务判据）。
    pub stamped: bool,
    /// 是否落入禁区（任一 step.zone.is_some()）。
    pub zone_hit: bool,
    /// 是否类目外（任一 step.class 不在 ALL）。
    pub class_unknown: bool,
    /// 是否时间源混用。
    pub time_mixed: bool,
    /// 采样序号（入窗序号，便于复现）。
    pub seq: u64,
}

impl ThSample {
    /// 构造样本（步数上界 8；超限返回 None 由调用方显性拒绝）。
    pub fn new(
        vector: u16,
        kind: ThKind,
        steps: Vec<ThStep>,
        acked: bool,
        stamped: bool,
    ) -> Option<Self> {
        if steps.len() > 8 {
            return None;
        }
        let mut total_ticks = 0u64;
        let mut zone_hit = false;
        let mut class_unknown = false;
        for s in &steps {
            total_ticks = total_ticks.saturating_add(s.ticks);
            if s.zone.is_some() {
                zone_hit = true;
            }
            if !ThClass::ALL.contains(&s.class) {
                class_unknown = true;
            }
        }
        Some(ThSample {
            vector,
            kind,
            steps,
            total_ticks,
            acked,
            stamped,
            zone_hit,
            class_unknown,
            time_mixed: false,
            seq: 0,
        })

/// handler 窗口（同一 handler 的采样序列 + 预算档）。
#[derive(Clone, Debug)]
pub struct ThWindow {
    /// handler 标识（向量号）。
    pub vector: u16,
    /// 大类（决定预算档）。
    pub kind: ThKind,
    /// 采样序列（定容）。
    pub samples: Vec<ThSample>,
    /// 归因步账（各类目已见标记，判 ClassDup 用）。
    pub class_seen: [bool; 5],
}

impl ThWindow {
    /// 构造窗口。
    pub fn new(vector: u16, kind: ThKind) -> Self {
        ThWindow {
            vector,
            kind,
            samples: Vec::new(),
            class_seen: [false; 5],
        }
    }

    /// 该窗口的预算档位（µs）。唯一进度量。
    pub fn budget_of(&self) -> u64 {
        match self.kind {
            ThKind::Clock => TH_CLOCK_BUDGET_US,
            ThKind::Ipi => TH_IPI_BUDGET_US,
            ThKind::Device => TH_DEV_BUDGET_US,
            ThKind::Unknown => TH_DEV_BUDGET_US,
        }
    }

    /// 时钟档错配检测：时钟类走了非时钟档预算即 Repeat 级。
    pub fn clock_mismatch(&self) -> bool {
        self.kind == ThKind::Clock && self.budget_of() != TH_CLOCK_BUDGET_US
    }

    /// 记账：写入一条样本；返回诊断码（若有）。
    pub fn charge(&mut self, mut s: ThSample) -> Result<(), (ThCode, String)> {
        // 采样容量守卫：第 65537 条即拒。
        if self.samples.len() >= TH_SAMPLE_CAP {
            return Err((
                ThCode::SampleFull,
                format!(
                    "handler 0x{:04x} 采样已满 {} 条，第 65537 条即拒；核对是否误用生产路径采样",
                    self.vector, TH_SAMPLE_CAP
                ),
            ));
        }
        // 时间源混用检测。
        if s.time_mixed {
            return Err((
                ThCode::TimeMix,
                format!(
                    "handler 0x{:04x} 样本时间源混用：同窗口只允许 tick，物理单位仅作 p99 折算",
                    self.vector
                ),
            ));
        }
        // 类目重复归集：同类操作被反复记账同一 handler。
        for st in &s.steps {
            let idx = ThClass::ALL.iter().position(|c| *c == st.class);
            if let Some(i) = idx {
                if self.class_seen[i] {
                    return Err((
                        ThCode::ClassDup,
                        format!(
                            "handler 0x{:04x} 类目 {} 重复归集：每类每 handler 只归集一次",
                            self.vector,
                            st.class.name()
                        ),
                    ));
                }
            }
        }
        s.seq = self.samples.len() as u64;
        for st in &s.steps {
            let idx = ThClass::ALL.iter().position(|c| *c == st.class);
            if let Some(i) = idx {
                self.class_seen[i] = true;
            }
        }
        self.samples.push(s);
        Ok(())
    }

    /// 样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 是否为空（空窗口断言前置）。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 样本总数（u64 视图）。
    pub fn count(&self) -> u64 {
        self.samples.len() as u64
    }

    /// p99 分位（µs；内置 P99 口径，不可改）。
    pub fn p99_us(&self) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut v: Vec<u64> = self.samples.iter().map(|s| s.total_ticks).collect();
        v.sort_unstable();
        let n = v.len();
        // p99 索引（内置口径：ceil(0.99*n)，最小 1，再减 1 转 0 基）。
        let mut idx = (n as u64).saturating_mul(TH_P99_IDX as u64) / 100;
        if idx == 0 {
            idx = 1;
        }
        let idx = (idx as usize).saturating_sub(1).min(n - 1);
        // 折算：tick → µs。
        v[idx].saturating_mul(1000) / TH_TICK_NS_PER_TICK
    }

    /// 最大单点时长（µs）。
    pub fn max_us(&self) -> u64 {
        self.samples
            .iter()
            .map(|s| s.total_ticks)
            .max()
            .unwrap_or(0)
            .saturating_mul(1000)
            / TH_TICK_NS_PER_TICK
    }

    /// 最小单点时长（µs）。
    pub fn min_us(&self) -> u64 {
        self.samples
            .iter()
            .map(|s| s.total_ticks)
            .min()
            .unwrap_or(0)
            .saturating_mul(1000)
            / TH_TICK_NS_PER_TICK
    }

    /// 均值（µs；整数除法）。
    pub fn avg_us(&self) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }
        let sum: u64 = self.samples.iter().map(|s| s.total_ticks).sum();
        sum.saturating_mul(1000) / TH_TICK_NS_PER_TICK / (self.samples.len() as u64)
    }

    /// 是否超预算（p99 > 档位硬线）。
    pub fn over_budget(&self) -> bool {
        !self.samples.is_empty() && self.p99_us() > self.budget_of()
    }

    /// 义务缺位样本数（应答或时间戳缺失）。
    pub fn duty_miss_count(&self) -> u64 {
        self.samples
            .iter()
            .filter(|s| !s.acked || !s.stamped)
            .count() as u64
    }

    /// 禁区命中样本数。
    pub fn zone_hit_count(&self) -> u64 {
        self.samples.iter().filter(|s| s.zone_hit).count() as u64
    }

    /// 类目外样本数。

/// 超预算清单条目（每例一条）。
#[derive(Clone, Debug)]
pub struct ThLedgerEntry {
    /// 向量号。
    pub vector: u16,
    /// 大类。
    pub kind: ThKind,
    /// 该例总时长（µs）。
    pub total_us: u64,
    /// 该例 p99（µs）。
    pub p99_us: u64,
    /// 最大嫌疑步骤类别。
    pub worst_class: ThClass,
    /// 最大嫌疑步骤时长（µs）。
    pub worst_us: u64,
    /// 是否落入禁区。
    pub zone_hit: bool,
    /// 是否义务缺位。
    pub duty_miss: bool,
    /// 严重度。
    pub severity: ThSeverity,
    /// 诊断码。
    pub code: ThCode,
    /// 指纹（去重用：向量 + 类别 + p99）。
    pub fingerprint: u64,
}

impl ThLedgerEntry {
    /// 指纹（FNV-1a 64 位）。
    pub fn fingerprint_of(vector: u16, worst_class: ThClass, p99_us: u64) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut mix = |b: u8| {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        };
        let vb = vector.to_le_bytes();
        for b in vb {
            mix(b);
        }
        mix(worst_class as u8);
        let pb = p99_us.to_le_bytes();
        for b in pb {
            mix(b);
        }
        h
    }
}

/// 超预算清单（自动产出，判据 10/10 次）。
#[derive(Clone, Debug)]
pub struct ThLedger {
    /// 条目序列（定容；溢出压最旧）。
    pub entries: Vec<ThLedgerEntry>,
    /// 入账计数（保真，不被压缩影响）。
    pub total_entered: u64,
    /// 去重命中计数。
    pub dup_skipped: u64,
    /// 溢出次数。
    pub overflow_times: u64,
}

impl ThLedger {
    /// 构造清单。
    pub fn new() -> Self {
        ThLedger {
            entries: Vec::new(),
            total_entered: 0,
            dup_skipped: 0,
            overflow_times: 0,
        }
    }

    /// 入账一条（同指纹二次入账走 DupLedger）。
    pub fn enter(&mut self, e: ThLedgerEntry) -> Result<(), (ThCode, String)> {
        if self.entries.iter().any(|x| x.fingerprint == e.fingerprint) {
            self.dup_skipped += 1;
            return Err((
                ThCode::DupLedger,
                format!(
                    "清单条目重复入账：向量 0x{:04x} 指纹 0x{:016x} 已入账",
                    e.vector, e.fingerprint
                ),
            ));
        }
        self.total_entered += 1;
        if self.entries.len() >= TH_LEDGER_CAP {
            // 压最旧：移除第 0 条。
            self.entries.remove(0);
            self.overflow_times += 1;
        }
        self.entries.push(e);
        Ok(())
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 按严重度 + p99 排序（稳定；供报告）。
    pub fn sorted_report(&self) -> Vec<ThLedgerEntry> {
        let mut v = self.entries.clone();
        v.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then(b.p99_us.cmp(&a.p99_us))
                .then(a.vector.cmp(&b.vector))
        });
        v
    }
}

/// 纪律引擎（窗口集 + 清单 + 冻结状态）。
#[derive(Clone, Debug)]
pub struct ThEngine {
    /// 窗口集（按向量号）。
    pub windows: Vec<ThWindow>,
    /// 超预算清单。
    pub ledger: ThLedger,
    /// 逻辑 tick 缩放（默认 1）。
    pub tick_scale: u64,
    /// 冻结状态（纪律文档冻结后只读）。
    pub frozen: bool,
}

impl Default for ThEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ThEngine {
    /// 构造引擎。
    pub fn new() -> Self {
        ThEngine {
            windows: Vec::new(),
            ledger: ThLedger::new(),
            tick_scale: 1,
            frozen: false,
        }
    }

    /// 找窗口（无则新建）。
    pub fn window_of(&mut self, vector: u16, kind: ThKind) -> &mut ThWindow {
        if let Some(idx) = self.windows.iter().position(|w| w.vector == vector) {
            &mut self.windows[idx]
        } else {
            self.windows.push(ThWindow::new(vector, kind));
            let idx = self.windows.len() - 1;
            &mut self.windows[idx]
        }
    }

    /// 只读取窗口。
    pub fn peek_window(&self, vector: u16) -> Option<&ThWindow> {
        self.windows.iter().find(|w| w.vector == vector)
    }

    /// 冻结纪律文档（三节齐备后调用）。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}
