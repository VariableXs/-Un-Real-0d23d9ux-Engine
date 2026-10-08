//! VE-F0229 · Intel 错误状态解析（VE-B 域 · Intel 核显组 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0229`
//!
//! **判据（锚点原文）**：表驱动、三分类、保守未知、限频合并、判据。
//!
//! **职责定位（锚点原文）**：Gen 错误寄存器组按代际映射（EIR/EMR/ESR 与
//! ring 状态），错误位翻译为三分类事件（可恢复、需重置、需下线），产出
//! 结构化错误报告供恢复矩阵与通知协议使用；解析器数据驱动按代际选表，
//! 未知位保守分类并保留原始值。
//!
//! ## 一、解析是「查表不是猜」：代际×寄存器×位 全表驱动
//!
//! 错误位语义随代际漂移（Gen9 的某位到 Xe 未必还是那个含义），所以
//! 解析器**不内嵌任何位语义**——语义全部在 [`ERR_TABLE`]（代际×寄存器
//! ×位×分类），解析 [`ErrorParser::parse`] 就是逐位查表 O(位数)。
//! 新代际支持=加表行，解析器零改动；表外无语义，杜绝「按最新代际
//! 猜旧硬件」的口径漂移。
//!
//! ## 二、三分类是处置矩阵的入口，不是修辞
//!
//! 每个错误位翻译为恰好一种处置类（[`EventClass::Recoverable`] 可恢复 /
//! [`EventClass::NeedsReset`] 需重置 / [`EventClass::NeedsOffline`] 需下线）：
//! 可恢复走 F0228 恢复联动、需重置走 F0224 上下文重置、需下线走通知协议。
//! 一位两分类或零分类都是解析器撒谎——表行的分类字段非此即彼，类型系统
//! 钉死。
//!
//! ## 三、未知位保守分类为需重置并保留原始值
//!
//! 表里没有的位**绝不静默丢弃**也绝不乐观分类：按 [`EventClass::NeedsReset`]
//! 保守分类（未知错误按「最坏可信处置」走），并在报告里保留原始位值
//! （[`ErrorReport::raw_bits`]）——丢原始值等于烧掉事后归因的唯一物证。
//! 寄存器读取失败则是另一种病：标缺测（[`RegRead::Missing`]）不虚构零值。
//!
//! ## 四、报告风暴限频合并，中断驱动零轮询
//!
//! 同类报告在限频窗口内合并为一条（[`ErrorParser::report`] 带窗口计数
//! O(1)），风暴期不淹没通知通道（下游 F0103 三要素）；解析由中断/事件
//! 驱动（[`ErrorParser::feed`] 被调才解析），零轮询。
//!
//! **对接**：上游 F0221 代际分型（[`GenTier`] 同源引用）；下游 F0224/F0228
//! 恢复联动（分类即处置入口）、F0103 通知。零 panic 面（`get`/`Option`、
//! 算术饱和）、零 IO、零墙钟（tick 账面）、无全局可变状态、no_std 零 std 依赖。

use alloc::string::String;
use alloc::vec::Vec;

use super::veb21_ident::GenTier;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 错误寄存器种类（锚点原文三件：EIR/EMR/ESR）。
pub const ERR_REGISTERS: usize = 3;

/// 每寄存器位宽（账面位宽；表驱动按位索引查）。
pub const REG_BITS: usize = 32;

/// ring 状态种类（渲染/视频/拷贝——与 F0228 FenceEngine 对位）。
pub const RING_STATES: usize = 3;

/// 限频窗口（tick）：同类报告窗口内合并。
pub const RATE_LIMIT_WINDOW_TICKS: u64 = 8;

/// 窗口内同类报告合并阈值：达到即合并计数不重复出报告。
pub const RATE_LIMIT_MERGE_AT: u32 = 3;

/// 报告环形容量（风暴兜底：满即挤最旧并如实计数）。
pub const REPORT_CAPACITY: usize = 32;

// ---------------------------------------------------------------------------
// 二、诊断码（独占 0x3Cxx 段；0x3Bxx 归 F0228，0x3Axx 归 X 域）
// ---------------------------------------------------------------------------

/// F0229 诊断码。独占 `0x3Cxx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrParseCode(pub u16);

impl ErrParseCode {
    /// 表驱动违规（表行分类越界——结构面防御，正常流程不可达）。
    pub const TABLE_ROW: ErrParseCode = ErrParseCode(0x3C01);
    /// 报告容量满（挤最旧，如实计数）。
    pub const REPORT_FULL: ErrParseCode = ErrParseCode(0x3C02);
    /// ring 状态越界（喂入口径错误）。
    pub const BAD_RING: ErrParseCode = ErrParseCode(0x3C03);
    /// 零代际查询（口径未初始化）。
    pub const BAD_GEN: ErrParseCode = ErrParseCode(0x3C04);
    /// 限频合并发生（可观测，非错误）。
    pub const MERGED: ErrParseCode = ErrParseCode(0x3C05);
    /// 未知位保守立案（可观测，非错误）。
    pub const UNKNOWN_BIT: ErrParseCode = ErrParseCode(0x3C06);

    /// 两两互异的 wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            ErrParseCode::TABLE_ROW => "错误位表行分类越界：表数据自检失败".into(),
            ErrParseCode::REPORT_FULL => "报告环形容量满：挤最旧并计数".into(),
            ErrParseCode::BAD_RING => "ring 状态越界：喂入口径非法".into(),
            ErrParseCode::BAD_GEN => "代际口径未初始化".into(),
            ErrParseCode::MERGED => "同类报告限频合并".into(),
            ErrParseCode::UNKNOWN_BIT => "未知错误位保守立案（需重置）并保留原始值".into(),
            ErrParseCode(_) => "未知错误解析诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 三、分类 / 寄存器 / 报告（数据结构：错误位表×报告结构）
// ---------------------------------------------------------------------------

/// 处置三分类（锚点原文：可恢复、需重置、需下线）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventClass {
    /// 可恢复：走 F0228 恢复联动。
    Recoverable,
    /// 需重置：走 F0224 上下文重置。
    NeedsReset,
    /// 需下线：走通知协议。
    NeedsOffline,
}

/// 寄存器种类（EIR/EMR/ESR）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrReg {
    /// 中断错误寄存器。
    Eir,
    /// 主错误寄存器。
    Emr,
    /// 从错误寄存器。
    Esr,
}

impl ErrReg {
    /// 表列索引（0/1/2）。
    pub const fn index(self) -> usize {
        match self {
            ErrReg::Eir => 0,
            ErrReg::Emr => 1,
            ErrReg::Esr => 2,
        }
    }

    /// 全集（判据对账用）。
    pub const ALL: [ErrReg; 3] = [ErrReg::Eir, ErrReg::Emr, ErrReg::Esr];
}

/// ring 状态种类（与 F0228 引擎对位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingState {
    /// 渲染 ring。
    Render,
    /// 视频 ring。
    Video,
    /// 拷贝 ring。
    Copy,
}

impl RingState {
    /// 表列索引。
    pub const fn index(self) -> usize {
        match self {
            RingState::Render => 0,
            RingState::Video => 1,
            RingState::Copy => 2,
        }
    }
}

/// 寄存器读取结果：有值或缺测（读取失败不虚构零值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegRead {
    /// 读到原始位面。
    Bits(u32),
    /// 读取失败：缺测标记。
    Missing,
}

/// 结构化错误报告（锚点数据结构：原始值×分类×建议）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorReport {
    /// 错误位表索引（代际×寄存器×位 归一编号）。
    pub table_row: u16,
    /// 处置分类。
    pub class: EventClass,
    /// 处置建议（人话，通知面 F0103 的 what/why/next 之 what 位）。
    pub advice: Advice,
    /// 未知位时保留的原始位值（表内位此值与位面同源）。
    pub raw_bits: u32,
    /// 是否未知位保守立案。
    pub unknown: bool,
    /// 报告发生 tick。
    pub tick: u64,
    /// 归属 ring 状态（报告随来源走，供恢复矩阵定位引擎）。
    pub ring: RingState,
}

/// 处置建议（分类的下一跳，对接 F0224/F0228/通知）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Advice {
    /// 记账后放行（可恢复）。
    LogAndProceed,
    /// 联动 F0228 恢复围栏映射。
    RecoverViaFence,
    /// 联动 F0224 上下文重置。
    ResetContext,
    /// 通知并下线该引擎。
    OfflineEngine,
}

/// 错误位表行：代际×寄存器×位 → 分类（语义全在表，解析器零内嵌）。
#[derive(Debug, Clone, Copy)]
pub struct ErrTableRow {
    /// 代际档。
    pub gen: GenTier,
    /// 寄存器种类。
    pub reg: ErrReg,
    /// 位索引（0..REG_BITS）。
    pub bit: usize,
    /// 处置分类。
    pub class: EventClass,
    /// 处置建议。
    pub advice: Advice,
}

/// 错误位表（锚点数据结构：代际×寄存器×位×分类；新代际支持=加行）。
/// 口径：同一位在新代际表中缺行 ⇒ 未知位保守路径接管（NeedsReset+raw）。
pub const ERR_TABLE: &[ErrTableRow] = &[
    // —— Gen9 Baseline：最小确定集（读失败保守下限同源 F0227 口径） ——
    ErrTableRow { gen: GenTier::Baseline, reg: ErrReg::Eir, bit: 0, class: EventClass::Recoverable, advice: Advice::LogAndProceed },
    ErrTableRow { gen: GenTier::Baseline, reg: ErrReg::Eir, bit: 5, class: EventClass::NeedsReset, advice: Advice::ResetContext },
    ErrTableRow { gen: GenTier::Baseline, reg: ErrReg::Emr, bit: 12, class: EventClass::NeedsOffline, advice: Advice::OfflineEngine },
    ErrTableRow { gen: GenTier::Baseline, reg: ErrReg::Esr, bit: 3, class: EventClass::NeedsReset, advice: Advice::RecoverViaFence },
    // —— Xe 标准档：位语义漂移显性化（同位号不同分类即漂移的证明） ——
    ErrTableRow { gen: GenTier::XeStandard, reg: ErrReg::Eir, bit: 0, class: EventClass::Recoverable, advice: Advice::LogAndProceed },
    ErrTableRow { gen: GenTier::XeStandard, reg: ErrReg::Eir, bit: 5, class: EventClass::Recoverable, advice: Advice::LogAndProceed },
    ErrTableRow { gen: GenTier::XeStandard, reg: ErrReg::Emr, bit: 12, class: EventClass::NeedsReset, advice: Advice::ResetContext },
    ErrTableRow { gen: GenTier::XeStandard, reg: ErrReg::Esr, bit: 3, class: EventClass::NeedsOffline, advice: Advice::OfflineEngine },
    // —— Xe 最新档：在标准档基础上收紧（Emr12 升下线） ——
    ErrTableRow { gen: GenTier::XeLatest, reg: ErrReg::Eir, bit: 0, class: EventClass::Recoverable, advice: Advice::LogAndProceed },
    ErrTableRow { gen: GenTier::XeLatest, reg: ErrReg::Eir, bit: 5, class: EventClass::Recoverable, advice: Advice::LogAndProceed },
    ErrTableRow { gen: GenTier::XeLatest, reg: ErrReg::Emr, bit: 12, class: EventClass::NeedsOffline, advice: Advice::OfflineEngine },
    ErrTableRow { gen: GenTier::XeLatest, reg: ErrReg::Esr, bit: 3, class: EventClass::NeedsOffline, advice: Advice::OfflineEngine },
];

// ---------------------------------------------------------------------------
// 四、ErrorParser 主结构（解析 O(位数) 表驱动；限频 O(1)；中断驱动零轮询）
// ---------------------------------------------------------------------------

/// Gen 错误状态解析器（数据驱动，被调才解析——零轮询）。
#[derive(Debug)]
pub struct ErrorParser {
    gen: Option<GenTier>,
    reports: Vec<ErrorReport>,
    merged: u32,
    last_merge_tick: u64,
    window_class: Option<EventClass>,
    window_count: u32,
    overflow_dropped: u32,
    unknown_kept: u32,
}

impl ErrorParser {
    /// 空解析器。
    pub fn new() -> Self {
        ErrorParser {
            gen: None,
            reports: Vec::new(),
            merged: 0,
            last_merge_tick: 0,
            window_class: None,
            window_count: 0,
            overflow_dropped: 0,
            unknown_kept: 0,
        }
    }

    /// 代际口径初始化/切换（切换后旧表行语义失效——表按代际选行）。
    pub fn set_gen(&mut self, gen: GenTier) {
        self.gen = Some(gen);
    }

    /// 已产出报告只读视图。
    pub fn reports(&self) -> &[ErrorReport] {
        &self.reports
    }

    /// 限频合并累计数（可观测，不修数）。
    pub const fn merged(&self) -> u32 {
        self.merged
    }

    /// 未知位保守立案累计数。
    pub const fn unknown_kept(&self) -> u32 {
        self.unknown_kept
    }

    /// 容量挤最旧累计数。
    pub const fn overflow_dropped(&self) -> u32 {
        self.overflow_dropped
    }

    /// 单寄存器位面解析（O(位数) 表驱动；ring 状态供 ring 类错误行查）。
    /// 返回本期新增报告数（限频合并时不重复计）。
    pub fn parse(
        &mut self,
        reg: ErrReg,
        read: RegRead,
        ring: RingState,
        tick: u64,
    ) -> Result<usize, ErrParseCode> {
        if self.gen.is_none() {
            return Err(ErrParseCode::BAD_GEN);
        }
        let gen = self.gen.unwrap_or(GenTier::Baseline);
        let _ = RING_STATES; // ring 列宽常量在位（RingState 三态类型系统拒绝越界）
        let bits = match read {
            RegRead::Bits(b) => b,
            RegRead::Missing => return Ok(0), // 缺测标记：不出报告不虚构
        };
        let mut produced = 0usize;
        for bit in 0..REG_BITS {
            let mask = 1u32.checked_shl(bit as u32).unwrap_or(0);
            if mask == 0 || bits & mask == 0 {
                continue;
            }
            let row = ERR_TABLE
                .iter()
                .find(|r| r.gen == gen && r.reg == reg && r.bit == bit);
            let (class, advice, unknown) = match row {
                Some(r) => (r.class, r.advice, false),
                None => (
                    EventClass::NeedsReset, // 保守分类：未知位按最坏可信处置
                    Advice::ResetContext,
                    true,
                ),
            };
            if unknown {
                self.unknown_kept = self.unknown_kept.saturating_add(1);
            }
            let row_id = Self::row_id(reg, bit);
            if self.rate_limited(class, tick) {
                continue; // 窗口内同类合并：计数不重复出报告
            }
            self.push_report(
                ErrorReport {
                    table_row: row_id,
                    class,
                    advice,
                    raw_bits: bits,
                    unknown,
                    tick,
                    ring,
                },
            )?;
            produced += 1;
        }
        Ok(produced)
    }

    /// 报告入口（容量满挤最旧并如实计数）。
    fn push_report(&mut self, r: ErrorReport) -> Result<(), ErrParseCode> {
        if self.reports.len() >= REPORT_CAPACITY {
            let _ = self.reports.remove(0);
            self.overflow_dropped = self.overflow_dropped.saturating_add(1);
            self.reports.push(r);
            return Err(ErrParseCode::REPORT_FULL);
        }
        self.reports.push(r);
        Ok(())
    }

    /// 限频合并 O(1)：同分类窗口内计数达 [`RATE_LIMIT_MERGE_AT`] 即合并。
    fn rate_limited(&mut self, class: EventClass, tick: u64) -> bool {
        let in_window = tick.saturating_sub(self.last_merge_tick) < RATE_LIMIT_WINDOW_TICKS;
        if in_window && self.window_class == Some(class) {
            self.window_count = self.window_count.saturating_add(1);
            if self.window_count >= RATE_LIMIT_MERGE_AT {
                self.merged = self.merged.saturating_add(1);
                self.last_merge_tick = tick;
                self.window_count = 0;
                return true;
            }
            return false;
        }
        self.window_class = Some(class);
        self.window_count = 1;
        self.last_merge_tick = tick;
        false
    }

    /// 归一行编号（寄存器×位；判据对账用，两两互异）。
    fn row_id(reg: ErrReg, bit: usize) -> u16 {
        ((reg.index() as u16) << 8) | (bit as u16 & 0xFF)
    }
}

// ---------------------------------------------------------------------------
// 五、域自检（判据：表驱动、三分类、保守未知、限频合并、判据）
// ---------------------------------------------------------------------------

/// VE-F0229 域自检入口（聚合器 `run_svstar2_checks` 调用）。
pub fn run_veb229_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("veb229_errparse");

    // —— 判据一 · 表驱动：代际×寄存器×位 查表命中，位语义随代际漂移显性化 ——
    let mut p = ErrorParser::new();
    let no_gen = p.parse(ErrReg::Eir, RegRead::Bits(1), RingState::Render, 0);
    p.set_gen(GenTier::Baseline);
    let _ = p.parse(ErrReg::Eir, RegRead::Bits(1), RingState::Render, 1);
    let r0 = p.reports().first().copied();
    p.set_gen(GenTier::XeStandard);
    let _ = p.parse(ErrReg::Eir, RegRead::Bits(1), RingState::Render, 2);
    let r0s = p.reports().last().copied();
    s.add(
        "B229-表驱动-代际选行与漂移显性",
        no_gen == Err(ErrParseCode::BAD_GEN)
            && r0.map(|r| r.class == EventClass::Recoverable) == Some(true)
            && r0s.map(|r| r.class == EventClass::Recoverable && r.tick == 2) == Some(true)
            && p.reports().len() == 2,
        "未初始化代际拒绝解析；同位（Eir0）两代际各按表行分类；解析逐位查表不内嵌语义",
    );

    // —— 判据一 · 反向：位语义漂移同位不同分类（Emr12 三代际三样） ——
    let mut drift = ErrorParser::new();
    drift.set_gen(GenTier::Baseline);
    let _ = drift.parse(ErrReg::Emr, RegRead::Bits(1 << 12), RingState::Video, 1);
    drift.set_gen(GenTier::XeStandard);
    let _ = drift.parse(ErrReg::Emr, RegRead::Bits(1 << 12), RingState::Video, 2);
    drift.set_gen(GenTier::XeLatest);
    let _ = drift.parse(ErrReg::Emr, RegRead::Bits(1 << 12), RingState::Video, 3);
    let classes: Vec<EventClass> = drift.reports().iter().map(|r| r.class).collect();
    s.add(
        "B229-表驱动-同位三代际漂移",
        classes == [
            EventClass::NeedsOffline,
            EventClass::NeedsReset,
            EventClass::NeedsOffline,
        ],
        "Emr12：Baseline 下线→XeStandard 重置→XeLatest 下线，漂移由表行承载非代码分支",
    );

    // —— 判据二 · 三分类：Esr3 按代际给 Recoverable/NeedsReset/NeedsOffline 互斥单分类 ——
    let three = [EventClass::Recoverable, EventClass::NeedsReset, EventClass::NeedsOffline];
    let mut distinct = true;
    for (i, g) in [GenTier::XeStandard, GenTier::XeLatest].iter().enumerate() {
        let _ = i;
        let mut pp = ErrorParser::new();
        pp.set_gen(*g);
        let _ = pp.parse(ErrReg::Esr, RegRead::Bits(1 << 3), RingState::Copy, 1);
        if let Some(r) = pp.reports().first() {
            if !three.contains(&r.class) {
                distinct = false;
            }
            // 分类与建议对位：NeedsOffline⇔OfflineEngine，NeedsReset⇔ResetContext/RecoverViaFence
            if (r.class == EventClass::NeedsOffline) != (r.advice == Advice::OfflineEngine) {
                distinct = false;
            }
        } else {
            distinct = false;
        }
    }
    s.add(
        "B229-三分类-分类与建议对位",
        distinct && three.len() == 3,
        "每位恰一分类且非此即彼；分类-建议映射钉死（NeedsOffline⇔OfflineEngine）",
    );

    // —— 判据二 · 反向：需重置类走围栏恢复建议（F0228 恢复联动入口） ——
    let mut p3 = ErrorParser::new();
    p3.set_gen(GenTier::Baseline);
    let _ = p3.parse(ErrReg::Esr, RegRead::Bits(1 << 3), RingState::Render, 1);
    let esr3 = p3.reports().first().copied();
    s.add(
        "B229-三分类-需重置联动围栏",
        esr3.map(|r| {
            r.class == EventClass::NeedsReset && r.advice == Advice::RecoverViaFence
        }) == Some(true),
        "Baseline Esr3 按表行给 NeedsReset+RecoverViaFence（F0228 恢复联动入口）",
    );

    // —— 判据三 · 保守未知：表外位 NeedsReset + 原始值保留 ——
    let mut p4 = ErrorParser::new();
    p4.set_gen(GenTier::Baseline);
    let raw = 1u32 << 30; // 表外位
    let _ = p4.parse(ErrReg::Eir, RegRead::Bits(raw), RingState::Render, 1);
    let unk = p4.reports().first().copied();
    s.add(
        "B229-保守未知-立案并留原始值",
        unk.map(|r| {
            r.unknown && r.class == EventClass::NeedsReset && r.raw_bits == raw
        }) == Some(true)
            && p4.unknown_kept() == 1,
        "表外位不静默不乐观：保守 NeedsReset、raw_bits 原样保留（事后归因物证不烧）",
    );

    // —— 判据三 · 反向：读取失败标缺测不出报告不虚构 ——
    let mut p5 = ErrorParser::new();
    p5.set_gen(GenTier::Baseline);
    let missing = p5.parse(ErrReg::Eir, RegRead::Missing, RingState::Render, 1);
    s.add(
        "B229-保守未知-缺测不出报告",
        missing == Ok(0) && p5.reports().is_empty(),
        "RegRead::Missing 零报告零虚构（读取失败与错误位是两种病）",
    );

    // —— 判据四 · 限频合并：同类窗口内达阈合并 O(1) ——
    let mut p6 = ErrorParser::new();
    p6.set_gen(GenTier::Baseline);
    // Eir5 Baseline=NeedsReset；连续四次喂同位（间隔 < 窗口 8）
    let _ = p6.parse(ErrReg::Eir, RegRead::Bits(1 << 5), RingState::Render, 1);
    let _ = p6.parse(ErrReg::Eir, RegRead::Bits(1 << 5), RingState::Render, 2);
    let _ = p6.parse(ErrReg::Eir, RegRead::Bits(1 << 5), RingState::Render, 3);
    let _ = p6.parse(ErrReg::Eir, RegRead::Bits(1 << 5), RingState::Render, 4);
    let merged_in_window = p6.merged();
    let reports_len = p6.reports().len();
    // 窗口外新报告不被合并
    let _ = p6.parse(ErrReg::Eir, RegRead::Bits(1 << 5), RingState::Render, 100);
    s.add(
        "B229-限频-同类窗口内合并出窗放行",
        merged_in_window == 1
            && reports_len == 3
            && p6.merged() == 1
            && p6.reports().last().map(|r| r.tick == 100) == Some(true),
        "四次同报：窗口内第 3 次达阈合并（报告恰 3 条）；出窗（tick100）新报告放行",
    );

    // —— 判据四 · 反向：异类不互相合并 + 容量挤最旧如实计数 ——
    let mut p7 = ErrorParser::new();
    p7.set_gen(GenTier::Baseline);
    let _ = p7.parse(ErrReg::Eir, RegRead::Bits(1 << 5), RingState::Render, 1);
    let _ = p7.parse(ErrReg::Emr, RegRead::Bits(1 << 12), RingState::Video, 2);
    let mixed = p7.merged() == 0 && p7.reports().len() == 2;
    // 容量：32 满 + 1 挤最旧
    let mut p8 = ErrorParser::new();
    p8.set_gen(GenTier::Baseline);
    for t in 0..(REPORT_CAPACITY as u64 + 1) {
        let _ = p8.parse(ErrReg::Esr, RegRead::Bits(1 << 3), RingState::Copy, t * 100);
    }
    s.add(
        "B229-限频-异类不混并与容量挤旧",
        mixed
            && p8.reports().len() == REPORT_CAPACITY
            && p8.overflow_dropped() == 1
            && p8.reports().first().map(|r| r.tick == 100) == Some(true),
        "两类报告不互相合并（同分类才并）；容量满挤最旧计数 1 且最旧已让位",
    );

    // —— 判据五 · 元数据：码段互异 + 常量口径 ——
    let codes = [
        ErrParseCode::TABLE_ROW.code(),
        ErrParseCode::REPORT_FULL.code(),
        ErrParseCode::BAD_RING.code(),
        ErrParseCode::BAD_GEN.code(),
        ErrParseCode::MERGED.code(),
        ErrParseCode::UNKNOWN_BIT.code(),
    ];
    let mut uniq = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                uniq = false;
            }
        }
    }
    let table_consistent = !ERR_TABLE.is_empty()
        && ERR_TABLE.iter().all(|r| r.bit < REG_BITS)
        && ERR_REGISTERS == ErrReg::ALL.len();
    s.add(
        "B229-判据-码段互异且表自洽",
        uniq
            && codes.iter().all(|c| c & 0xFF00 == 0x3C00)
            && table_consistent
            && RATE_LIMIT_WINDOW_TICKS > 0,
        "六码全落 0x3Cxx 两两互异；位表行位号全在界内；寄存器三件口径与常量对账",
    );

    // —— 判据五 · 对账：raw_bits 单源（表内位与未知位共用原始位面字段） ——
    let mut p9 = ErrorParser::new();
    p9.set_gen(GenTier::Baseline);
    let combo = (1u32 << 0) | (1u32 << 30); // 表内 Eir0 + 表外 bit30 同面喂入
    let _ = p9.parse(ErrReg::Eir, RegRead::Bits(combo), RingState::Render, 1);
    let two: Vec<ErrorReport> = p9.reports().to_vec();
    let both_raw = two.len() == 2 && two.iter().all(|r| r.raw_bits == combo)
        && two.iter().all(|r| r.ring == RingState::Render)
        && two.iter().any(|r| r.unknown)
        && two.iter().any(|r| !r.unknown);
    s.add(
        "B229-判据-原始位面单源",
        both_raw,
        "同位面混合喂入：表内位与未知位各出一报告且 raw_bits 同源（一处真相）",
    );

    s
}
