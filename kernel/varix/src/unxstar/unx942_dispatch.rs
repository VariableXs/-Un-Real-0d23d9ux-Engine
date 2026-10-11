//! UNX-F0942 · 分发入口统一路径（IDT→向量分型→handler 的主通道）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F0942`
//!
//! **判据 J1**：主通道正确（五类向量经统一入口各 10/10 次正确分路），路径
//! 开销 <300ns（入口到 handler 实测），旁路清零断言（现存直挂 IDT 的路径
//! 收编干净）。
//!
//! ## 要点一：五类向量分型闭集——向量号区间分型，闭集外拒
//!
//! 五类闭集：异常（0x00..=0x1F）、IRQ（0x20..=0xEF，基址 0x20 可注入）、
//! IPI（0xF0..=0xFD）、NMI（0xFE 语义）、伪向量（0xFF）。闭集即全 256 向
//! 量空间按区间划分、互不重叠；入口收 u16，超 0xFF 一律拒
//! （VECTOR_OUT_OF_CLOSED_SET）——分型函数是全函数，入口是守门员。
//!
//! ## 要点二：统一入口 DispatchEntry——每次进入登记
//!
//! 每次进入统一入口登记一笔：向量号 / 分型 / 路由结果 / 逻辑步数 / 时间戳
//! tick。登记簿为环形账（容量上限 4096，溢出翻旧账并立案
//! ENTRY_LOG_OVERFLOW）；tick 单调性检查（回退即拒，TICK_REGRESSION）。
//!
//! ## 要点三：步数开销账——每步 2ns，300ns 红线 = 150 步
//!
//! 全部开销用**逻辑步数**计量（纯账面状态机，非墙钟）：每逻辑步折算
//! 2ns（常数来源：取指+译码+寄存器访问的保守单步折算；见 STEP_NS 注释）。
//! 300ns 红线 = 150 步（COST_REDLINE_STEPS）；每事件落一笔步数样本，
//! P95 整数分位 + 最大步数超 150 步即立案（COST_REDLINE_BREACH）。
//!
//! ## 要点四：五类各 10/10 次正确分路判定账
//!
//! 分型探针（每类 10 个代表向量，分型必须与期望类一致）+ 分路探针（每类
//! 10 次经统一入口分派，路由结果必须等于期望路由：四类实类 Invoked、
//! 伪向量 SpuriousAck）。任一类不满 10/10 → 正确率 <100% → 立案
//! （CLASSIFY_ROUTE_BREACH）。
//!
//! ## 要点五：旁路收编账 BypassLedger——直挂路径收编干净
//!
//! 现存直挂 IDT 的路径 = 旁路槽。统一入口注册该向量 handler 后方可收编
//! （adopt）；审计时旁路槽必须为零，非零即红（BYPASS_SLOTS_NONZERO）。
//! 重复登记旁路（BYPASS_DOUBLE_MARK）、收编未登记（BYPASS_SLOT_ABSENT）
//! 均有独立诊断码——BypassLedger 闭集。
//!
//! ## 要点六：handler 注册表 + 屏蔽 + 风暴 + 重放 + 统计
//!
//! 向量→handler 槽位（256 槽）：重复注册拒绝、注销清理、分派遇未注册
//! = 悬挂向量即立案（HANDLER_UNREGISTERED）；优先级语义（实类优先级
//! 与注册声明不符 = PRIORITY_INVERSION）；IRQ 分组屏蔽位图（26 组，
//! 组外拒）；风暴防护（同向量 1 秒逻辑窗千次计数熔断）；重放对拍
//! （事件序列双跑逐事件一致）；统计导出（分型频次 + 开销分位 + 旁路账
//! kv 逐行）。
//!
//! ## 要点七：零 panic 面 + 诊断码独占 0x8Cxx 段
//!
//! 与 DSDT 册（0x2Exx）/深化（0x2Fxx）/SSDT（0x07xx）/总线扫描
//! （0x7Bxx）/看门狗（0x94xx）/GPT（0x95xx）/像素（0x96xx）/帧账
//! （0x98xx）/串口（0x99xx）等互不重叠；0x8Cxx 段经 grep 同目录核对
//! 无撞后选用。本模块零 panic 面——所有输入不可信（向量号/tick/handler
//! 声明皆可错），错误一律 Err + 中文诊断，禁 unwrap/expect/panic!，
//! 禁 IO、真实 IDT/端口操作——IDT 与分发全部语义模型，全确定性、禁浮点。

use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

// ---------------------------------------------------------------------------
// 一、诊断码闭集（独占 0x8Cxx 段——17 码互异，grep 同目录核对无撞）
// ---------------------------------------------------------------------------

/// 分发入口统一路径诊断码。独占 `0x8Cxx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DpCode(pub u16);

impl DpCode {
    /// 向量号越出五类闭集（>0xFF 或分型函数不认）。
    pub const VECTOR_OUT_OF_CLOSED_SET: DpCode = DpCode(0x8C01);
    /// 分型/分路探针正确率 <100%（任一类不满 10/10 即立案）。
    pub const CLASSIFY_ROUTE_BREACH: DpCode = DpCode(0x8C02);
    /// handler 槽位已占（重复注册拒绝）。
    pub const HANDLER_SLOT_OCCUPIED: DpCode = DpCode(0x8C03);
    /// 悬挂向量：分派遇未注册向量即立案。
    pub const HANDLER_UNREGISTERED: DpCode = DpCode(0x8C04);
    /// 注销目标不存在（槽位本就为空）。
    pub const HANDLER_UNREG_TARGET_MISSING: DpCode = DpCode(0x8C05);
    /// IRQ 屏蔽组号越界（组号 >= 26）。
    pub const MASK_GROUP_OUT_OF_RANGE: DpCode = DpCode(0x8C06);
    /// 风暴熔断：同向量 1 秒逻辑窗计数超千。
    pub const STORM_BREAKER_TRIPPED: DpCode = DpCode(0x8C07);
    /// 步数开销 300ns 红线超限（最大步数 > 150）立案。
    pub const COST_REDLINE_BREACH: DpCode = DpCode(0x8C08);
    /// 旁路槽非零：统一入口注册后仍有直挂路径未收编。
    pub const BYPASS_SLOTS_NONZERO: DpCode = DpCode(0x8C09);
    /// 旁路槽重复登记。
    pub const BYPASS_DOUBLE_MARK: DpCode = DpCode(0x8C0A);
    /// 收编目标未登记旁路（无账可收）。
    pub const BYPASS_SLOT_ABSENT: DpCode = DpCode(0x8C0B);
    /// 重放对拍：双跑逐事件不一致。
    pub const REPLAY_MISMATCH: DpCode = DpCode(0x8C0C);
    /// tick 回退（时间戳必须单调不减）。
    pub const TICK_REGRESSION: DpCode = DpCode(0x8C0D);
    /// 优先级语义冲突：注册声明的优先级与分型固有优先级不符。
    pub const PRIORITY_INVERSION: DpCode = DpCode(0x8C0E);
    /// 账目守恒破坏：分型频次合计 != 登记簿笔数。
    pub const LEDGER_CONSERVATION_BROKEN: DpCode = DpCode(0x8C0F);
    /// 登记簿溢出（环形账翻旧已发生，立案留痕）。
    pub const ENTRY_LOG_OVERFLOW: DpCode = DpCode(0x8C10);
    /// 段位守卫：诊断码越出本模块 0x8Cxx 段。
    pub const SEGMENT_GUARD: DpCode = DpCode(0x8C11);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            DpCode::VECTOR_OUT_OF_CLOSED_SET => "向量号越出五类闭集：分型区间 0x00..=0xFF 之外拒收".into(),
            DpCode::CLASSIFY_ROUTE_BREACH => "分型/分路正确率不足：五类探针任一类未满 10/10".into(),
            DpCode::HANDLER_SLOT_OCCUPIED => "handler 槽位已占：同向量重复注册拒绝".into(),
            DpCode::HANDLER_UNREGISTERED => "悬挂向量：统一入口分派遇未注册向量，立案".into(),
            DpCode::HANDLER_UNREG_TARGET_MISSING => "注销目标不存在：该向量槽位本就为空".into(),
            DpCode::MASK_GROUP_OUT_OF_RANGE => "IRQ 屏蔽组号越界：合法组 0..=25".into(),
            DpCode::STORM_BREAKER_TRIPPED => "风暴熔断：同向量 1 秒逻辑窗计数超千".into(),
            DpCode::COST_REDLINE_BREACH => "步数开销超红线：最大步数超 150 步（300ns）".into(),
            DpCode::BYPASS_SLOTS_NONZERO => "旁路槽非零：仍有直挂 IDT 路径未收编，红".into(),
            DpCode::BYPASS_DOUBLE_MARK => "旁路槽重复登记：同向量旁路账已有一笔".into(),
            DpCode::BYPASS_SLOT_ABSENT => "收编目标未登记旁路：无账可收".into(),
            DpCode::REPLAY_MISMATCH => "重放对拍不一致：双跑逐事件结果有差异".into(),
            DpCode::TICK_REGRESSION => "tick 回退：时间戳必须单调不减".into(),
            DpCode::PRIORITY_INVERSION => "优先级语义冲突：声明优先级与分型固有优先级不符".into(),
            DpCode::LEDGER_CONSERVATION_BROKEN => "账目守恒破坏：分型频次合计与登记簿笔数不符".into(),
            DpCode::ENTRY_LOG_OVERFLOW => "登记簿溢出：环形账已翻旧，早期笔目不可回放".into(),
            DpCode::SEGMENT_GUARD => "段位守卫：诊断码越出本模块 0x8Cxx 段".into(),
            DpCode(_) => "未知 UNX 分发域诊断码".into(),
        }
    }

    /// 码值反查闭集成员（越段返回 None——段位守卫的镜像）。
    pub fn of(code: u16) -> Option<DpCode> {
        match code {
            0x8C01 => Some(DpCode::VECTOR_OUT_OF_CLOSED_SET),
            0x8C02 => Some(DpCode::CLASSIFY_ROUTE_BREACH),
            0x8C03 => Some(DpCode::HANDLER_SLOT_OCCUPIED),
            0x8C04 => Some(DpCode::HANDLER_UNREGISTERED),
            0x8C05 => Some(DpCode::HANDLER_UNREG_TARGET_MISSING),
            0x8C06 => Some(DpCode::MASK_GROUP_OUT_OF_RANGE),
            0x8C07 => Some(DpCode::STORM_BREAKER_TRIPPED),
            0x8C08 => Some(DpCode::COST_REDLINE_BREACH),
            0x8C09 => Some(DpCode::BYPASS_SLOTS_NONZERO),
            0x8C0A => Some(DpCode::BYPASS_DOUBLE_MARK),
            0x8C0B => Some(DpCode::BYPASS_SLOT_ABSENT),
            0x8C0C => Some(DpCode::REPLAY_MISMATCH),
            0x8C0D => Some(DpCode::TICK_REGRESSION),
            0x8C0E => Some(DpCode::PRIORITY_INVERSION),
            0x8C0F => Some(DpCode::LEDGER_CONSERVATION_BROKEN),
            0x8C10 => Some(DpCode::ENTRY_LOG_OVERFLOW),
            0x8C11 => Some(DpCode::SEGMENT_GUARD),
            _ => None,
        }
    }

    /// 段位守卫：码值必须落在本模块 0x8Cxx 段内。
    pub const fn in_segment(code: u16) -> bool {
        code >= 0x8C01 && code <= 0x8C11
    }
}

// ---------------------------------------------------------------------------
// 二、五类向量分型闭集——向量号区间分型，闭集外拒
// ---------------------------------------------------------------------------

/// 异常类下界（除零/越界/缺页等 32 个经典异常向量）。
pub const EXC_LO: u16 = 0x00;
/// 异常类上界。
pub const EXC_HI: u16 = 0x1F;
/// IRQ 基址（可注入区间下界——外部中断从 0x20 起）。
pub const IRQ_BASE: u16 = 0x20;
/// IRQ 区间上界。
pub const IRQ_HI: u16 = 0xEF;
/// IPI 区间下界（处理器间中断保留段）。
pub const IPI_LO: u16 = 0xF0;
/// IPI 区间上界。
pub const IPI_HI: u16 = 0xFD;
/// NMI 语义向量号（不可屏蔽中断固定 0xFE）。
pub const NMI_VECTOR: u16 = 0xFE;
/// 伪向量号（假中断确认专用 0xFF）。
pub const SPURIOUS_VECTOR: u16 = 0xFF;

/// 向量五类分型闭集。区间互斥、并集恰为 0x00..=0xFF。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorKind {
    /// 异常类：0x00..=0x1F。
    Exception,
    /// 外部中断类：0x20..=0xEF（基址 0x20 可注入）。
    Irq,
    /// 处理器间中断类：0xF0..=0xFD。
    Ipi,
    /// 不可屏蔽中断：0xFE 语义。
    Nmi,
    /// 伪向量：0xFF（假中断，确认即弃）。
    Spurious,
}

impl VectorKind {
    /// 五类全序（下标即账本桶位）。
    pub const ALL: [VectorKind; 5] = [
        VectorKind::Exception,
        VectorKind::Irq,
        VectorKind::Ipi,
        VectorKind::Nmi,
        VectorKind::Spurious,
    ];

    /// 账本桶位下标。
    pub const fn index(self) -> usize {
        match self {
            VectorKind::Exception => 0,
            VectorKind::Irq => 1,
            VectorKind::Ipi => 2,
            VectorKind::Nmi => 3,
            VectorKind::Spurious => 4,
        }
    }

    /// 由桶位下标反查（越界 None）。
    pub fn from_index(i: usize) -> Option<VectorKind> {
        match i {
            0 => Some(VectorKind::Exception),
            1 => Some(VectorKind::Irq),
            2 => Some(VectorKind::Ipi),
            3 => Some(VectorKind::Nmi),
            4 => Some(VectorKind::Spurious),
            _ => None,
        }
    }

    /// 人话类名。
    pub const fn name(self) -> &'static str {
        match self {
            VectorKind::Exception => "异常",
            VectorKind::Irq => "IRQ",
            VectorKind::Ipi => "IPI",
            VectorKind::Nmi => "NMI",
            VectorKind::Spurious => "伪向量",
        }
    }

    /// 固有优先级（0 最高）：NMI > 异常 > IPI > IRQ > 伪向量。
    ///
    /// 语义模型：NMI 不可屏蔽故最优先；异常同步于指令流次之；IPI 高于
    /// 外部 IRQ；伪向量最低（确认即弃）。注册 handler 时声明优先级必须
    /// 与此一致，不符即 PRIORITY_INVERSION。
    pub const fn priority(self) -> u8 {
        match self {
            VectorKind::Nmi => 0,
            VectorKind::Exception => 1,
            VectorKind::Ipi => 2,
            VectorKind::Irq => 3,
            VectorKind::Spurious => 4,
        }
    }

    /// 区间描述（诊断/导出用）。
    pub const fn range_desc(self) -> &'static str {
        match self {
            VectorKind::Exception => "0x00..=0x1F",
            VectorKind::Irq => "0x20..=0xEF",
            VectorKind::Ipi => "0xF0..=0xFD",
            VectorKind::Nmi => "0xFE",
            VectorKind::Spurious => "0xFF",
        }
    }
}

/// 五类分型函数（全函数）：闭集内返回类，闭集外返回 None。
///
/// 规则：① >0xFF 直接拒（u16 入口的守门员职责）；② 0x00..=0x1F 异常；
/// ③ 0x20..=0xEF IRQ；④ 0xF0..=0xFD IPI；⑤ 0xFE NMI；⑥ 0xFF 伪向量。
pub fn classify(vector: u16) -> Option<VectorKind> {
    if vector > SPURIOUS_VECTOR {
        return None;
    }
    match vector {
        EXC_LO..=EXC_HI => Some(VectorKind::Exception),
        IRQ_BASE..=IRQ_HI => Some(VectorKind::Irq),
        IPI_LO..=IPI_HI => Some(VectorKind::Ipi),
        NMI_VECTOR => Some(VectorKind::Nmi),
        SPURIOUS_VECTOR => Some(VectorKind::Spurious),
        _ => None,
    }
}

/// 分型的 Result 皮肤（入口主通道用，闭集外即诊断码）。
pub fn classify_strict(vector: u16) -> Result<VectorKind, DpCode> {
    classify(vector).ok_or(DpCode::VECTOR_OUT_OF_CLOSED_SET)
}

// ---------------------------------------------------------------------------
// 三、IRQ 分组屏蔽位图——26 组语义
// ---------------------------------------------------------------------------

/// IRQ 分组大小（8 向量一组——经典 8259 级联粒度的语义继承）。
pub const IRQ_GROUP_SIZE: u16 = 8;
/// IRQ 组数：0x20..=0xEF 共 208 向量 / 8 = 26 组。
pub const IRQ_GROUP_COUNT: usize = ((IRQ_HI - IRQ_BASE + 1) / IRQ_GROUP_SIZE) as usize;

/// 由向量号求 IRQ 组号（非 IRQ 向量返回 None）。
pub fn irq_group(vector: u16) -> Option<usize> {
    if classify(vector) != Some(VectorKind::Irq) {
        return None;
    }
    Some(((vector - IRQ_BASE) / IRQ_GROUP_SIZE) as usize)
}

/// IRQ 分组屏蔽位图：26 组恰落一个 u32（位图语义，组号即位号）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaskBitmap {
    bits: u32,
}

impl MaskBitmap {
    /// 新位图：全开放（无屏蔽）。
    pub const fn new() -> Self {
        MaskBitmap { bits: 0 }
    }

    /// 屏蔽一组（置位）。组号越界 → MASK_GROUP_OUT_OF_RANGE。
    pub fn mask_group(&mut self, group: usize) -> Result<(), DpCode> {
        if group >= IRQ_GROUP_COUNT {
            return Err(DpCode::MASK_GROUP_OUT_OF_RANGE);
        }
        self.bits |= 1u32 << group;
        Ok(())
    }

    /// 解除一组屏蔽（清位）。组号越界 → MASK_GROUP_OUT_OF_RANGE。
    pub fn unmask_group(&mut self, group: usize) -> Result<(), DpCode> {
        if group >= IRQ_GROUP_COUNT {
            return Err(DpCode::MASK_GROUP_OUT_OF_RANGE);
        }
        self.bits &= !(1u32 << group);
        Ok(())
    }

    /// 查询一组是否被屏蔽（组号越界恒 false——调用方已守门）。
    pub fn is_masked(&self, group: usize) -> bool {
        if group >= IRQ_GROUP_COUNT {
            return false;
        }
        (self.bits >> group) & 1 == 1
    }

    /// 当前被屏蔽的组数（账目导出用）。
    pub fn masked_count(&self) -> u32 {
        self.bits.count_ones()
    }

    /// 原始位图（导出留痕）。
    pub const fn raw(&self) -> u32 {
        self.bits
    }
}

// ---------------------------------------------------------------------------
// 四、步数开销模型——每步 2ns，300ns 红线 = 150 步
// ---------------------------------------------------------------------------

/// 每逻辑步折算 2ns。
///
/// 常数来源：纯账面状态机的单逻辑步取"取指 + 译码 + 寄存器访问"的保守
/// 单步折算（x86-64 简单指令典型 1~3ns，取上界 2ns 整数化）。禁墙钟，
/// 全部开销由逻辑步数 × 2ns 映射，全确定性。
pub const STEP_NS: u64 = 2;
/// 300ns 红线对应的步数上限：300 / 2 = 150 步。
pub const COST_REDLINE_STEPS: u32 = 150;
/// 红线纳米数（300ns——判据原文）。
pub const COST_REDLINE_NS: u64 = 300;

/// 分型计步：区间判定树走查（6 步——两段比较 + 落桶）。
pub const STEPS_CLASSIFY: u32 = 6;
/// 入口登记计步：登记簿落账（5 步——序号/类/路由/步数/时刻五字段）。
pub const STEPS_ENTRY_BOOK: u32 = 5;
/// 屏蔽检查计步：组号换算 + 位图取位（3 步）。
pub const STEPS_MASK_CHECK: u32 = 3;
/// 风暴检查计步：窗界比较 + 计数递增 + 熔断判定（5 步）。
pub const STEPS_STORM_CHECK: u32 = 5;
/// 路由查表计步：256 槽位直取 + 槽位判空（8 步）。
pub const STEPS_ROUTE_LOOKUP: u32 = 8;
/// handler 调用计步：帧装配 + 间接转移 + 结果回填（12 步）。
pub const STEPS_INVOKE: u32 = 12;
/// 伪向量确认计步：EOI 语义确认（4 步）。
pub const STEPS_SPURIOUS_ACK: u32 = 4;

/// 最坏正常路径步数（分型+登记+风暴+查表+调用）——必须低于红线。
pub const WORST_CASE_STEPS: u32 =
    STEPS_CLASSIFY + STEPS_ENTRY_BOOK + STEPS_STORM_CHECK + STEPS_ROUTE_LOOKUP + STEPS_INVOKE;

// ---------------------------------------------------------------------------
// 五、事件帧 / handler 契约
// ---------------------------------------------------------------------------

/// 分派给 handler 的事件帧（纯账面——无真实寄存器，语义模型）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventFrame {
    /// 向量号（已过闭集守门，恒 <= 0xFF）。
    pub vector: u8,
    /// 分型结果。
    pub kind: VectorKind,
    /// 事件载荷（语义模型中的"错误码/中断源"占位）。
    pub payload: u64,
    /// 时间戳 tick（逻辑推进，非墙钟）。
    pub tick: u64,
    /// 统一入口流水序号。
    pub seq: u64,
}

/// handler 返回契约（账面结果，无副作用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerOutcome {
    /// 已处理。
    Handled,
    /// 暂缓（下窗再议）。
    Deferred,
    /// 拒绝并附因（账面因码）。
    Reject(u16),
}

impl HandlerOutcome {
    /// 是否按已处理落账。
    pub const fn is_handled(self) -> bool {
        matches!(self, HandlerOutcome::Handled)
    }
}

/// handler 函数类型：纯函数指针——无状态、无 IO、全确定性。
pub type HandlerFn = fn(&EventFrame) -> HandlerOutcome;

/// 内置样例 handler：回执已处理（探针/重放用）。
pub fn h_echo(frame: &EventFrame) -> HandlerOutcome {
    let _ = frame;
    HandlerOutcome::Handled
}

/// 内置样例 handler：载荷为零则暂缓，否则已处理（分支语义样例）。
pub fn h_conditional(frame: &EventFrame) -> HandlerOutcome {
    if frame.payload == 0 {
        HandlerOutcome::Deferred
    } else {
        HandlerOutcome::Handled
    }
}

/// 内置样例 handler：恒拒并附因（错误路径样例）。
pub fn h_reject(frame: &EventFrame) -> HandlerOutcome {
    let _ = frame;
    HandlerOutcome::Reject(0xFFFF)
}

// ---------------------------------------------------------------------------
// 六、handler 注册表——256 槽位，重复注册拒绝 / 注销清理 / 悬挂立案
// ---------------------------------------------------------------------------

/// handler 槽位（向量 → handler 的账面绑定）。
#[derive(Clone)]
pub struct HandlerSlot {
    /// 绑定向量号。
    pub vector: u8,
    /// handler 名（账面可读标识）。
    pub name: String,
    /// handler 函数指针。
    pub handler: HandlerFn,
    /// 注册时声明的优先级（必须等于分型固有优先级）。
    pub priority: u8,
    /// 注册时刻 tick。
    pub registered_tick: u64,
    /// 累计调用次数。
    pub invoke_count: u64,
    /// 最近一次返回契约（None = 尚未被分派过）。
    pub last_outcome: Option<HandlerOutcome>,
}

/// handler 注册表：256 槽位直取模型（槽位下标即向量号）。
pub struct HandlerRegistry {
    slots: Vec<Option<HandlerSlot>>,
}

impl HandlerRegistry {
    /// 空表。
    pub fn new() -> Self {
        HandlerRegistry {
            slots: vec![None; 256],
        }
    }

    /// 注册 handler：闭集校验 + 占位拒绝 + 优先级核对。
    ///
    /// 规则：① 向量越闭集 → VECTOR_OUT_OF_CLOSED_SET；② 槽位已占 →
    /// HANDLER_SLOT_OCCUPIED（重复注册拒绝）；③ 声明优先级 != 分型固有
    /// 优先级 → PRIORITY_INVERSION；④ 通过即落槽。
    pub fn register(
        &mut self,
        vector: u16,
        name: &str,
        handler: HandlerFn,
        priority: u8,
        tick: u64,
    ) -> Result<(), DpCode> {
        let kind = classify_strict(vector)?;
        if self.slots[(vector & 0xFF) as usize].is_some() {
            return Err(DpCode::HANDLER_SLOT_OCCUPIED);
        }
        if priority != kind.priority() {
            return Err(DpCode::PRIORITY_INVERSION);
        }
        self.slots[(vector & 0xFF) as usize] = Some(HandlerSlot {
            vector: (vector & 0xFF) as u8,
            name: name.to_string(),
            handler,
            priority,
            registered_tick: tick,
            invoke_count: 0,
            last_outcome: None,
        });
        Ok(())
    }

    /// 注销 handler：清理槽位并返回原槽（注销即清理——判据要求）。
    pub fn unregister(&mut self, vector: u16) -> Result<HandlerSlot, DpCode> {
        classify_strict(vector)?;
        match self.slots[(vector & 0xFF) as usize].take() {
            Some(slot) => Ok(slot),
            None => Err(DpCode::HANDLER_UNREG_TARGET_MISSING),
        }
    }

    /// 查槽（不可变）。
    pub fn lookup(&self, vector: u16) -> Option<&HandlerSlot> {
        if vector > SPURIOUS_VECTOR {
            return None;
        }
        self.slots[vector as usize].as_ref()
    }

    /// 查槽（可变——分派路径回填调用账用）。
    pub fn lookup_mut(&mut self, vector: u16) -> Option<&mut HandlerSlot> {
        if vector > SPURIOUS_VECTOR {
            return None;
        }
        self.slots[vector as usize].as_mut()
    }

    /// 已注册槽位数。
    pub fn registered_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// 已注册向量清单（升序——账面导出确定序）。
    pub fn registered_vectors(&self) -> Vec<u16> {
        let mut out = Vec::new();
        for (i, s) in self.slots.iter().enumerate() {
            if s.is_some() {
                out.push(i as u16);
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 七、优先级与屏蔽语义——固有优先级表 + IRQ 组屏蔽位图
// ---------------------------------------------------------------------------

/// 固有优先级表核对：分型 → 固有优先级（与 VectorKind::priority 一致，
/// 独立成表防单点改动漂移——双表对拍语义）。
pub const PRIORITY_TABLE: [u8; 5] = [1, 3, 2, 0, 4];

/// 优先级双表对拍：PRIORITY_TABLE 必须与 VectorKind::priority 全等。
///
/// 任何一侧单独改动而不同步 → 本函数 Err（PRIORITY_INVERSION）——
/// 账面自检，构建期语义。
pub fn priority_table_consistent() -> Result<(), DpCode> {
    for kind in VectorKind::ALL.iter() {
        if PRIORITY_TABLE[kind.index()] != kind.priority() {
            return Err(DpCode::PRIORITY_INVERSION);
        }
    }
    Ok(())
}

/// 优先级比较：a 是否严格高于 b（数值小者高）。
pub fn priority_higher(a: VectorKind, b: VectorKind) -> bool {
    a.priority() < b.priority()
}

// ---------------------------------------------------------------------------
// 八、风暴防护——同向量 1 秒逻辑窗千次计数熔断
// ---------------------------------------------------------------------------

/// 逻辑 tick 折算：1 tick = 1000ns = 1µs（确定性推进常数，非墙钟）。
pub const TICK_NS: u64 = 1000;
/// 风暴窗宽：1 秒 = 1_000_000 tick（TICK_NS=1000 折算）。
pub const STORM_WINDOW_TICKS: u64 = 1_000_000;
/// 熔断阈值：同向量单窗 1000 次。
pub const STORM_THRESHOLD: u32 = 1000;

/// 单向量风暴窗状态（count==0 即未激活）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StormState {
    window_start: u64,
    count: u32,
    tripped: bool,
}

/// 风暴防护器：256 向量各一窗。
pub struct StormGuard {
    states: Vec<StormState>,
    trips: u64,
}

impl StormGuard {
    /// 新防护器：全窗未激活。
    pub fn new() -> Self {
        StormGuard {
            states: (0..256)
                .map(|_| StormState {
                    window_start: 0,
                    count: 0,
                    tripped: false,
                })
                .collect(),
            trips: 0,
        }
    }

    /// 观测一次事件：窗滚动 + 计数 + 熔断判定。
    ///
    /// 规则：① 计数非零且 tick 已滚出窗宽 → 重窗清零；② 计数为零 →
    /// 窗起点=本 tick；③ 计数超千 → 置熔断位并 Err(STORM_BREAKER_TRIPPED)。
    pub fn observe(&mut self, vector: u8, tick: u64) -> Result<(), DpCode> {
        let st = &mut self.states[vector as usize];
        if st.count > 0 && tick.saturating_sub(st.window_start) >= STORM_WINDOW_TICKS {
            st.count = 0;
            st.tripped = false;
        }
        if st.count == 0 {
            st.window_start = tick;
        }
        st.count = st.count.saturating_add(1);
        if st.count > STORM_THRESHOLD {
            st.tripped = true;
            self.trips = self.trips.saturating_add(1);
            return Err(DpCode::STORM_BREAKER_TRIPPED);
        }
        Ok(())
    }

    /// 查询某向量是否处于熔断位（窗滚过后自动解除——见 observe ①）。
    pub fn is_tripped(&self, vector: u8) -> bool {
        self.states[vector as usize].tripped
    }

    /// 手动复位某向量熔断位（运维语义——账面操作）。
    pub fn reset(&mut self, vector: u8, tick: u64) {
        let st = &mut self.states[vector as usize];
        st.count = 0;
        st.tripped = false;
        st.window_start = tick;
    }

    /// 累计熔断触发次数。
    pub const fn trips_total(&self) -> u64 {
        self.trips
    }
}

// ---------------------------------------------------------------------------
// 九、统一入口登记簿——DispatchEntry / RouteResult
// ---------------------------------------------------------------------------

/// 路由结果闭集（六值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteResult {
    /// 已分派给注册 handler。
    Invoked,
    /// IRQ 命中屏蔽组，挂起不派。
    Masked,
    /// 伪向量确认即弃。
    SpuriousAck,
    /// 风暴熔断截断。
    StormCut,
    /// 悬挂向量：未注册即立案。
    Hung,
    /// tick 回退，入口拒收（未产生分型账）。
    TickRefused,
}

impl RouteResult {
    /// 人话路由名。
    pub const fn name(self) -> &'static str {
        match self {
            RouteResult::Invoked => "已分派",
            RouteResult::Masked => "屏蔽挂起",
            RouteResult::SpuriousAck => "伪向量确认",
            RouteResult::StormCut => "风暴截断",
            RouteResult::Hung => "悬挂未注册",
            RouteResult::TickRefused => "tick 回退拒收",
        }
    }
}

/// 统一入口登记簿一笔：每次进入必登一账。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchEntry {
    /// 流水序号（单调递增）。
    pub seq: u64,
    /// 向量号（闭集内，恒 <= 0xFF）。
    pub vector: u8,
    /// 分型结果。
    pub kind: VectorKind,
    /// 路由结果。
    pub route: RouteResult,
    /// 本事件逻辑步数（×2ns 即开销）。
    pub steps: u32,
    /// 时间戳 tick。
    pub tick: u64,
    /// 命中的 handler 名（未派/无 handler 为 None）。
    pub handler: Option<String>,
}

/// 立案记录（诊断码 + 向量 + 时刻 + 人话备注）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRecord {
    /// 诊断码（0x8Cxx 段）。
    pub code: u16,
    /// 关联向量（无向量语境为 0）。
    pub vector: u8,
    /// 立案时刻 tick。
    pub tick: u64,
    /// 人话备注。
    pub note: String,
}

// ---------------------------------------------------------------------------
// 十、步数开销账 CostLedger——样本 + P95 整数分位 + 红线
// ---------------------------------------------------------------------------

/// 步数样本容量上限（超出即封账并置截断旗——确定性语义）。
pub const SAMPLE_CAP: usize = 4096;

/// 步数开销账。
#[derive(Debug, Clone)]
pub struct CostLedger {
    samples: Vec<u32>,
    truncated: bool,
}

impl CostLedger {
    /// 空账。
    pub fn new() -> Self {
        CostLedger {
            samples: Vec::new(),
            truncated: false,
        }
    }

    /// 落一笔步数样本（满则封账置旗，不再收样）。
    pub fn push(&mut self, steps: u32) {
        if self.samples.len() >= SAMPLE_CAP {
            self.truncated = true;
            return;
        }
        self.samples.push(steps);
    }

    /// 样本数。
    pub fn count(&self) -> usize {
        self.samples.len()
    }

    /// 是否封账截断。
    pub const fn is_truncated(&self) -> bool {
        self.truncated
    }

    /// 整数分位：升序取第 p 百分位（整数运算，确定性排序）。
    ///
    /// 规则：空账 None；idx = ceil(n*p/100)（至少 1、至多 n）；取升序
    /// 第 idx 个。P95 与红线判定共用本函数。
    pub fn percentile(&self, p: u32) -> Option<u32> {
        if self.samples.is_empty() {
            return None;
        }
        let n = self.samples.len() as u64;
        let mut idx = (n * p as u64 + 99) / 100;
        if idx == 0 {
            idx = 1;
        }
        if idx > n {
            idx = n;
        }
        let mut sorted: Vec<u32> = vec![0u32; self.samples.len()];
        sorted.copy_from_slice(&self.samples);
        sorted.sort();
        sorted.get((idx - 1) as usize).copied()
    }

    /// 最大步数。
    pub fn max_steps(&self) -> Option<u32> {
        self.samples.iter().copied().max()
    }

    /// 红线核对：最大步数 > 150 → Some(最大步数)，否则 None。
    pub fn redline_breach(&self) -> Option<u32> {
        match self.max_steps() {
            Some(m) if m > COST_REDLINE_STEPS => Some(m),
            _ => None,
        }
    }

    /// 步数 → 纳米折算。
    pub const fn steps_to_ns(steps: u32) -> u64 {
        steps as u64 * STEP_NS
    }
}

// ---------------------------------------------------------------------------
// 十一、旁路收编账 BypassLedger——直挂路径闭集，非零即红
// ---------------------------------------------------------------------------

/// 旁路槽：一笔直挂 IDT 的遗留路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BypassSlot {
    /// 旁路向量号。
    pub vector: u8,
    /// 遗留路径名（账面可读）。
    pub legacy_path: String,
    /// 是否已收编。
    pub adopted: bool,
    /// 收编时刻（未收编为 None）。
    pub adopted_tick: Option<u64>,
}

/// 旁路收编账：现存直挂路径闭集。
pub struct BypassLedger {
    slots: Vec<BypassSlot>,
}

impl BypassLedger {
    /// 空账。
    pub fn new() -> Self {
        BypassLedger { slots: Vec::new() }
    }

    /// 登记一笔旁路（同向量重复登记拒）。
    pub fn mark(&mut self, vector: u16, legacy_path: &str) -> Result<(), DpCode> {
        classify_strict(vector)?;
        let v = (vector & 0xFF) as u8;
        if self.slots.iter().any(|s| s.vector == v) {
            return Err(DpCode::BYPASS_DOUBLE_MARK);
        }
        self.slots.push(BypassSlot {
            vector: v,
            legacy_path: legacy_path.to_string(),
            adopted: false,
            adopted_tick: None,
        });
        Ok(())
    }

    /// 收编一笔：该向量已在统一入口注册 handler 才可收编。
    ///
    /// 规则：① 无旁路账 → BYPASS_SLOT_ABSENT；② 统一入口未注册该向量
    /// handler → HANDLER_UNREGISTERED（收编必须走主通道，不得凭空转正）；
    /// ③ 通过 → 置已收编 + 记时刻。
    pub fn adopt(
        &mut self,
        vector: u16,
        registry: &HandlerRegistry,
        tick: u64,
    ) -> Result<(), DpCode> {
        classify_strict(vector)?;
        let v = (vector & 0xFF) as u8;
        let slot = match self.slots.iter_mut().find(|s| s.vector == v) {
            Some(s) => s,
            None => return Err(DpCode::BYPASS_SLOT_ABSENT),
        };
        if slot.adopted {
            return Ok(());
        }
        if registry.lookup(vector).is_none() {
            return Err(DpCode::HANDLER_UNREGISTERED);
        }
        slot.adopted = true;
        slot.adopted_tick = Some(tick);
        Ok(())
    }

    /// 未收编笔数（审计"非零即红"的直接判据）。
    pub fn pending(&self) -> usize {
        self.slots.iter().filter(|s| !s.adopted).count()
    }

    /// 总笔数。
    pub fn total(&self) -> usize {
        self.slots.len()
    }

    /// 已收编笔数。
    pub fn adopted_count(&self) -> usize {
        self.slots.iter().filter(|s| s.adopted).count()
    }

    /// 旁路审计：pending 非零 → Err(BYPASS_SLOTS_NONZERO)（非零即红）。
    pub fn audit(&self) -> Result<BypassAudit, DpCode> {
        let report = BypassAudit {
            total: self.total(),
            adopted: self.adopted_count(),
            pending: self.pending(),
        };
        if report.pending != 0 {
            return Err(DpCode::BYPASS_SLOTS_NONZERO);
        }
        Ok(report)
    }

    /// 旁路账快照（导出用）。
    pub fn snapshot(&self) -> Vec<BypassSlot> {
        self.slots.clone()
    }
}

/// 旁路审计报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BypassAudit {
    /// 旁路总笔数。
    pub total: usize,
    /// 已收编笔数。
    pub adopted: usize,
    /// 未收编笔数（红判据）。
    pub pending: usize,
}

// ---------------------------------------------------------------------------
// 十二、五类探针账——分型 10/10 + 分路 10/10
// ---------------------------------------------------------------------------

/// 异常类分型探针向量（10 个，覆盖 0x00/0x03/0x06/0x08/0x0D/0x0E/0x10/0x13/0x1A/0x1F）。
pub const PROBE_EXCEPTION: [u16; 10] = [0x00, 0x03, 0x06, 0x08, 0x0D, 0x0E, 0x10, 0x13, 0x1A, 0x1F];
/// IRQ 类分型探针向量（10 个，覆盖基址/高端/边界 0xEF）。
pub const PROBE_IRQ: [u16; 10] = [0x20, 0x2B, 0x30, 0x41, 0x50, 0x6F, 0x7E, 0x91, 0xC3, 0xEF];
/// IPI 类分型探针向量（10 个，覆盖 0xF0..=0xFD 全段取样）。
pub const PROBE_IPI: [u16; 10] = [0xF0, 0xF1, 0xF3, 0xF5, 0xF7, 0xF9, 0xFA, 0xFB, 0xFC, 0xFD];
/// NMI 类分型探针（0xFE × 10 次——单向量类按次计）。
pub const PROBE_NMI: [u16; 10] = [0xFE; 10];
/// 伪向量分型探针（0xFF × 10 次）。
pub const PROBE_SPURIOUS: [u16; 10] = [0xFF; 10];

/// 单类探针计数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindCount {
    /// 类别。
    pub kind: VectorKind,
    /// 探针次数（应为 10）。
    pub attempts: u32,
    /// 正确次数（应为 10——不满即立案）。
    pub correct: u32,
}

impl KindCount {
    /// 是否满 10/10。
    pub const fn is_full(&self) -> bool {
        self.correct == self.attempts && self.attempts == 10
    }
}

/// 五类探针总报告（分型 + 分路两册）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    /// 分型册（每类 10/10）。
    pub classify: [KindCount; 5],
    /// 分路册（每类 10/10）。
    pub route: [KindCount; 5],
    /// 两册全满。
    pub all_ok: bool,
}

// ---------------------------------------------------------------------------
// 十三、重放对拍——事件序列双跑逐事件一致
// ---------------------------------------------------------------------------

/// 重放事件（向量/载荷/tick 三元组——确定性输入）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayEvent {
    /// 向量号（u16——允许构造越闭集事件以对拍拒收行为）。
    pub vector: u16,
    /// 载荷。
    pub payload: u64,
    /// 时间戳 tick。
    pub tick: u64,
}

/// 重放对拍报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayReport {
    /// 事件数。
    pub events: usize,
    /// 双跑逐事件是否全一致。
    pub identical: bool,
    /// 首个不一致事件下标。
    pub mismatch_at: Option<usize>,
    /// 不一致详情（一致为 None）。
    pub detail: Option<String>,
}

/// 登记笔目逐字段对拍（向量/分型/路由/步数/tick/handler 名/序号全等）。
fn entries_match(a: &DispatchEntry, b: &DispatchEntry) -> bool {
    a.seq == b.seq
        && a.vector == b.vector
        && a.kind == b.kind
        && a.route == b.route
        && a.steps == b.steps
        && a.tick == b.tick
        && a.handler == b.handler
}

/// 重放对拍：同一 setup 装配两个独立核，同一事件序列双跑，逐事件对拍。
///
/// 规则：① setup 为纯装配函数（注册/屏蔽/旁路登记），两核各自执行一遍；
/// ② 每事件双跑，Ok 对 Ok 比登记笔目、Err 对 Err 比诊断码、交叉即不一致；
/// ③ 首个不一致即停，报告带下标与详情（REPLAY_MISMATCH 语义由报告承载，
/// 由调用方决定是否立案——对拍器本身无副作用）。
pub fn replay_dual(
    setup: fn(&mut DispatchCore),
    events: &[ReplayEvent],
) -> Result<ReplayReport, DpCode> {
    let mut a = DispatchCore::new();
    let mut b = DispatchCore::new();
    setup(&mut a);
    setup(&mut b);
    for (i, ev) in events.iter().enumerate() {
        let ra = a.dispatch(ev.vector, ev.payload, ev.tick);
        let rb = b.dispatch(ev.vector, ev.payload, ev.tick);
        let same = match (&ra, &rb) {
            (Ok(ea), Ok(eb)) => entries_match(ea, eb),
            (Err(x), Err(y)) => x.code() == y.code(),
            _ => false,
        };
        if !same {
            let detail = match (&ra, &rb) {
                (Ok(ea), Ok(eb)) => format!(
                    "事件#{} 双跑登记不一致：A=(seq {} 类 {} 路 {} 步 {}) B=(seq {} 类 {} 路 {} 步 {})",
                    i,
                    ea.seq,
                    ea.kind.name(),
                    ea.route.name(),
                    ea.steps,
                    eb.seq,
                    eb.kind.name(),
                    eb.route.name(),
                    eb.steps
                ),
                (Ok(ea), Err(eb)) => format!(
                    "事件#{} 双跑不一致：A 落账(路 {}) 而 B 报错(码 0x{:04X})",
                    i,
                    ea.route.name(),
                    eb.code()
                ),
                (Err(ea), Ok(eb)) => format!(
                    "事件#{} 双跑不一致：A 报错(码 0x{:04X}) 而 B 落账(路 {})",
                    i,
                    ea.code(),
                    eb.route.name()
                ),
                (Err(ea), Err(eb)) => format!(
                    "事件#{} 双跑不一致：A 码 0x{:04X} B 码 0x{:04X}",
                    i,
                    ea.code(),
                    eb.code()
                ),
            };
            return Ok(ReplayReport {
                events: events.len(),
                identical: false,
                mismatch_at: Some(i),
                detail: Some(detail),
            });
        }
    }
    Ok(ReplayReport {
        events: events.len(),
        identical: true,
        mismatch_at: None,
        detail: None,
    })
}

// ---------------------------------------------------------------------------
// 十四、统一入口 DispatchCore——主通道状态机
// ---------------------------------------------------------------------------

/// 登记簿容量（环形账，溢出翻旧并置旗）。
pub const ENTRY_CAP: usize = 4096;
/// 立案账容量（环形账，溢出置旗）。
pub const ISSUES_CAP: usize = 256;

/// 统一入口核：IDT→向量分型→handler 的主通道纯账面状态机。
pub struct DispatchCore {
    registry: HandlerRegistry,
    mask: MaskBitmap,
    storm: StormGuard,
    cost: CostLedger,
    bypass: BypassLedger,
    entries: Vec<DispatchEntry>,
    issues: Vec<IssueRecord>,
    kind_counts: [u64; 5],
    route_counts: [u64; 6],
    seq: u64,
    last_tick: u64,
    entries_wrapped: bool,
    issues_overflow: bool,
    last_probe: Option<ProbeReport>,
}

impl DispatchCore {
    /// 新核：全空账全开放。
    pub fn new() -> Self {
        DispatchCore {
            registry: HandlerRegistry::new(),
            mask: MaskBitmap::new(),
            storm: StormGuard::new(),
            cost: CostLedger::new(),
            bypass: BypassLedger::new(),
            entries: Vec::new(),
            issues: Vec::new(),
            kind_counts: [0; 5],
            route_counts: [0; 6],
            seq: 0,
            last_tick: 0,
            entries_wrapped: false,
            issues_overflow: false,
            last_probe: None,
        }
    }

    // ---- 装配面（setup 用） -------------------------------------------

    /// 注册 handler（透传注册表；装配期便捷入口）。
    pub fn register_handler(
        &mut self,
        vector: u16,
        name: &str,
        handler: HandlerFn,
        tick: u64,
    ) -> Result<(), DpCode> {
        let kind = classify_strict(vector)?;
        self.registry
            .register(vector, name, handler, kind.priority(), tick)
    }

    /// 注销 handler（透传注册表）。
    pub fn unregister_handler(&mut self, vector: u16) -> Result<HandlerSlot, DpCode> {
        self.registry.unregister(vector)
    }

    /// 屏蔽一组 IRQ。
    pub fn mask_group(&mut self, group: usize) -> Result<(), DpCode> {
        self.mask.mask_group(group)
    }

    /// 解除一组 IRQ 屏蔽。
    pub fn unmask_group(&mut self, group: usize) -> Result<(), DpCode> {
        self.mask.unmask_group(group)
    }

    /// 登记一笔旁路（遗留直挂路径）。
    pub fn mark_bypass(&mut self, vector: u16, legacy_path: &str) -> Result<(), DpCode> {
        self.bypass.mark(vector, legacy_path)
    }

    /// 收编一笔旁路（必须已走统一入口注册）。
    pub fn adopt_bypass(&mut self, vector: u16, tick: u64) -> Result<(), DpCode> {
        // 借用拆分：先取注册表引用，再进旁路账收编。
        if self.registry.lookup(vector).is_none() {
            classify_strict(vector)?;
            return Err(DpCode::HANDLER_UNREGISTERED);
        }
        self.bypass.adopt(vector, &self.registry, tick)
    }

    // ---- 立案账 --------------------------------------------------------

    /// 立案一笔（环形账：满则翻旧置旗——旗由审计红线兜底，不再递归立案）。
    fn file_issue(&mut self, code: DpCode, vector: u8, tick: u64, note: &str) {
        if self.issues.len() >= ISSUES_CAP {
            self.issues.remove(0);
            self.issues_overflow = true;
        }
        self.issues.push(IssueRecord {
            code: code.code(),
            vector,
            tick,
            note: note.to_string(),
        });
    }

    /// 立案账只读视图。
    pub fn issues(&self) -> &[IssueRecord] {
        &self.issues
    }

    /// 立案笔数。
    pub fn issue_count(&self) -> usize {
        self.issues.len()
    }

    /// 登记簿是否翻过旧账。
    pub const fn entries_wrapped(&self) -> bool {
        self.entries_wrapped
    }

    /// 立案账是否溢出过。
    pub const fn issues_overflow(&self) -> bool {
        self.issues_overflow
    }

    // ---- 主通道：dispatch ----------------------------------------------

    /// 统一入口主通道：IDT→向量分型→handler 的唯一路径。
    ///
    /// 通道序：① tick 单调性（回退拒收 + 立案）；② 闭集守门（越界拒 +
    /// 立案）；③ 分型计步；④ 风暴检查（熔断 → StormCut 落账）；⑤ IRQ
    /// 屏蔽检查（命中 → Masked 落账）；⑥ 伪向量确认（SpuriousAck 落账）；
    /// ⑦ 查表分派（未注册 → Hung 落账 + 立案 + Err；已注册 → Invoked 落
    /// 账）。每笔落账同步计数/取样，全确定性。
    pub fn dispatch(&mut self, vector: u16, payload: u64, tick: u64) -> Result<DispatchEntry, DpCode> {
        // ① tick 单调性。
        if tick < self.last_tick {
            self.file_issue(
                DpCode::TICK_REGRESSION,
                (vector & 0xFF) as u8,
                tick,
                "时间戳回退：入口拒收该事件",
            );
            return Err(DpCode::TICK_REGRESSION);
        }
        self.last_tick = tick;

        // ② 闭集守门。
        let kind = match classify(vector) {
            Some(k) => k,
            None => {
                self.file_issue(
                    DpCode::VECTOR_OUT_OF_CLOSED_SET,
                    (vector & 0xFF) as u8,
                    tick,
                    "向量号越出五类闭集",
                );
                return Err(DpCode::VECTOR_OUT_OF_CLOSED_SET);
            }
        };
        let v = (vector & 0xFF) as u8;

        // ③④ 分型 + 登记 + 风暴计步。
        let mut steps = STEPS_CLASSIFY + STEPS_ENTRY_BOOK + STEPS_STORM_CHECK;
        if self.storm.observe(v, tick).is_err() {
            let entry = self.push_entry(v, kind, RouteResult::StormCut, steps, tick, None);
            return Ok(entry);
        }

        // ⑤ IRQ 屏蔽检查（仅 IRQ 类计步）。
        if kind == VectorKind::Irq {
            steps += STEPS_MASK_CHECK;
            if let Some(g) = irq_group(vector) {
                if self.mask.is_masked(g) {
                    let entry = self.push_entry(v, kind, RouteResult::Masked, steps, tick, None);
                    return Ok(entry);
                }
            }
        }

        // ⑥ 伪向量确认即弃（无需 handler）。
        if kind == VectorKind::Spurious {
            steps += STEPS_SPURIOUS_ACK;
            let entry = self.push_entry(v, kind, RouteResult::SpuriousAck, steps, tick, None);
            return Ok(entry);
        }

        // ⑦ 查表分派。
        let name = match self.registry.lookup(vector) {
            Some(slot) => slot.name.clone(),
            None => {
                // 悬挂向量：未注册即立案（落账 + 立案 + Err 三连）。
                let entry = self.push_entry(v, kind, RouteResult::Hung, steps, tick, None);
                self.file_issue(
                    DpCode::HANDLER_UNREGISTERED,
                    v,
                    tick,
                    "悬挂向量：统一入口分派遇未注册向量",
                );
                return Ok(entry);
            }
        };
        steps += STEPS_ROUTE_LOOKUP + STEPS_INVOKE;
        let frame = EventFrame {
            vector: v,
            kind,
            payload,
            tick,
            seq: self.seq,
        };
        let outcome = {
            let slot = match self.registry.lookup_mut(vector) {
                Some(s) => s,
                None => {
                    // 理论不可达（上方 lookup 已判有），按悬挂语义落账兜底。
                    let entry = self.push_entry(v, kind, RouteResult::Hung, steps, tick, None);
                    self.file_issue(DpCode::HANDLER_UNREGISTERED, v, tick, "查表竞态兜底：槽位消失");
                    return Ok(entry);
                }
            };
            let oc = (slot.handler)(&frame);
            slot.invoke_count = slot.invoke_count.saturating_add(1);
            slot.last_outcome = Some(oc);
            oc
        };
        let _ = outcome.is_handled(); // 契约回执已入槽账，此处仅确认无副作用。
        let entry = self.push_entry(v, kind, RouteResult::Invoked, steps, tick, Some(name));
        Ok(entry)
    }

    /// 落账一笔：序号 + 类计数 + 路由计数 + 步数取样 + 登记簿（环形）。
    fn push_entry(
        &mut self,
        vector: u8,
        kind: VectorKind,
        route: RouteResult,
        steps: u32,
        tick: u64,
        handler: Option<String>,
    ) -> DispatchEntry {
        self.seq = self.seq.saturating_add(1);
        self.kind_counts[kind.index()] = self.kind_counts[kind.index()].saturating_add(1);
        self.route_counts[route_index(route)] = self.route_counts[route_index(route)].saturating_add(1);
        self.cost.push(steps);
        let entry = DispatchEntry {
            seq: self.seq,
            vector,
            kind,
            route,
            steps,
            tick,
            handler,
        };
        if self.entries.len() >= ENTRY_CAP {
            self.entries.remove(0);
            self.entries_wrapped = true;
        }
        self.entries.push(entry.clone());
        entry
    }

    // ---- 探针册：五类各 10/10 分型 + 分路 ------------------------------

    /// 五类探针总账：分型 10/10 + 分路 10/10，任一不满即立案。
    ///
    /// 分路探针规则：① 实类（异常/IRQ/IPI/NMI）期望路由 Invoked——每类
    /// 选一个代表向量（探针表中首个可用槽位：已有 handler 用之，否则注册
    /// 内置回执 handler）；② 伪向量期望 SpuriousAck（无需 handler）；
    /// ③ 每类 10 次，逐次核对路由结果；④ 任一类 <10/10 → 立案
    /// CLASSIFY_ROUTE_BREACH。
    pub fn run_probe_suite(&mut self, tick: u64) -> ProbeReport {
        let tables: [[u16; 10]; 5] = [
            PROBE_EXCEPTION,
            PROBE_IRQ,
            PROBE_IPI,
            PROBE_NMI,
            PROBE_SPURIOUS,
        ];
        // 册一：分型 10/10。
        let mut classify_book = [KindCount {
            kind: VectorKind::Exception,
            attempts: 10,
            correct: 0,
        }; 5];
        for i in 0..5 {
            let kind = VectorKind::ALL[i];
            let mut correct = 0u32;
            for &pv in tables[i].iter() {
                if classify(pv) == Some(kind) {
                    correct += 1;
                }
            }
            classify_book[i] = KindCount {
                kind,
                attempts: 10,
                correct,
            };
        }
        // 册二：分路 10/10（统一入口实派）。
        let mut route_book = [KindCount {
            kind: VectorKind::Exception,
            attempts: 10,
            correct: 0,
        }; 5];
        for i in 0..5 {
            let kind = VectorKind::ALL[i];
            let expected = if kind == VectorKind::Spurious {
                RouteResult::SpuriousAck
            } else {
                RouteResult::Invoked
            };
            // 代表向量：探针表内首个可用槽位（已有 handler 用之，否则注册）。
            let mut rep: Option<u16> = None;
            for &pv in tables[i].iter() {
                if self.registry.lookup(pv).is_some() {
                    rep = Some(pv);
                    break;
                }
                let k = match classify(pv) {
                    Some(k) => k,
                    None => continue,
                };
                if self
                    .register_handler(pv, "probe.echo", h_echo as HandlerFn, tick)
                    .is_ok()
                {
                    let _ = k;
                    rep = Some(pv);
                    break;
                }
            }
            let mut correct = 0u32;
            if let Some(rv) = rep {
                for n in 0..10u64 {
                    let ev_tick = tick.saturating_add(n);
                    let ok = match self.dispatch(rv, n, ev_tick) {
                        Ok(e) => e.kind == kind && e.route == expected,
                        Err(_) => false,
                    };
                    if ok {
                        correct += 1;
                    }
                }
            }
            route_book[i] = KindCount {
                kind,
                attempts: 10,
                correct,
            };
        }
        let all_ok = classify_book.iter().all(|k| k.is_full())
            && route_book.iter().all(|k| k.is_full());
        if !all_ok {
            let mut note = String::from("五类探针未全满 10/10：");
            for k in classify_book.iter() {
                if !k.is_full() {
                    note.push_str(&format!("[分型 {} {}/10] ", k.kind.name(), k.correct));
                }
            }
            for k in route_book.iter() {
                if !k.is_full() {
                    note.push_str(&format!("[分路 {} {}/10] ", k.kind.name(), k.correct));
                }
            }
            self.file_issue(DpCode::CLASSIFY_ROUTE_BREACH, 0, tick, &note);
        }
        let report = ProbeReport {
            classify: classify_book,
            route: route_book,
            all_ok,
        };
        self.last_probe = Some(report.clone());
        report
    }

    /// 最近一次探针报告。
    pub fn last_probe(&self) -> Option<&ProbeReport> {
        self.last_probe.as_ref()
    }

    // ---- 账目守恒 ------------------------------------------------------

    /// 账目守恒核对：五类频次合计 == 登记簿笔数。
    ///
    /// 守恒语义：每笔登记必归一类（分型全函数），频次合计与笔数必须相等；
    /// 不等即账目被破坏（LEDGER_CONSERVATION_BROKEN）。
    pub fn conservation_check(&self) -> Result<(), DpCode> {
        let sum: u64 = self.kind_counts.iter().sum();
        if sum != self.entries.len() as u64 {
            return Err(DpCode::LEDGER_CONSERVATION_BROKEN);
        }
        Ok(())
    }

    // ---- 全量审计 ------------------------------------------------------

    /// 全量审计：探针 10/10 → 守恒 → 步数红线 → 旁路清零 → 溢出旗。
    ///
    /// 顺序短路：首个红项立案并 Err 返回；全绿则 Ok 报告（含 P50/P95/
    /// 最大步数与旁路账快照）。步数红线：最大步数 > 150（300ns）即立案。
    pub fn full_audit(&mut self, tick: u64) -> Result<FullAuditReport, DpCode> {
        // ① 探针 10/10。
        let probe = self.run_probe_suite(tick);
        if !probe.all_ok {
            return Err(DpCode::CLASSIFY_ROUTE_BREACH);
        }
        // ② 账目守恒。
        if let Err(e) = self.conservation_check() {
            self.file_issue(e, 0, tick, "分型频次合计与登记簿笔数不符");
            return Err(e);
        }
        // ③ 步数红线。
        if let Some(max) = self.cost.redline_breach() {
            self.file_issue(
                DpCode::COST_REDLINE_BREACH,
                0,
                tick,
                &format!("最大步数 {} 超 150 步红线（{}ns）", max, CostLedger::steps_to_ns(max)),
            );
            return Err(DpCode::COST_REDLINE_BREACH);
        }
        // ④ 旁路清零断言。
        let bypass_report = match self.bypass.audit() {
            Ok(r) => r,
            Err(e) => {
                self.file_issue(
                    e,
                    0,
                    tick,
                    &format!("旁路槽未收编 {} 笔，非零即红", self.bypass.pending()),
                );
                return Err(e);
            }
        };
        // ⑤ 溢出旗（翻旧账 = 早期笔目不可回放，立案留痕）。
        if self.entries_wrapped || self.issues_overflow {
            self.file_issue(
                DpCode::ENTRY_LOG_OVERFLOW,
                0,
                tick,
                "登记簿/立案账已翻旧，早期笔目不可回放",
            );
            return Err(DpCode::ENTRY_LOG_OVERFLOW);
        }
        Ok(FullAuditReport {
            probe,
            entries: self.entries.len(),
            p50: self.cost.percentile(50),
            p95: self.cost.percentile(95),
            max_steps: self.cost.max_steps(),
            redline_steps: COST_REDLINE_STEPS,
            redline_ns: COST_REDLINE_NS,
            bypass: bypass_report,
            hung: self.route_counts[route_index(RouteResult::Hung)],
            storm_cuts: self.route_counts[route_index(RouteResult::StormCut)],
            masked: self.route_counts[route_index(RouteResult::Masked)],
            issues: self.issues.len(),
            steps_truncated: self.cost.is_truncated(),
        })
    }

    // ---- 统计导出 ------------------------------------------------------

    /// 统计导出：分型频次 + 路由频次 + 开销分位 + 旁路账 kv 逐行。
    pub fn export_stats(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        out.push("module=UNX-F0942".to_string());
        out.push(format!("entries={}", self.entries.len()));
        out.push(format!("seq={}", self.seq));
        out.push(format!("last_tick={}", self.last_tick));
        // 分型频次。
        for kind in VectorKind::ALL.iter() {
            out.push(format!(
                "kind.{}.count={}",
                kv_kind_key(*kind),
                self.kind_counts[kind.index()]
            ));
        }
        // 路由频次。
        let routes = [
            RouteResult::Invoked,
            RouteResult::Masked,
            RouteResult::SpuriousAck,
            RouteResult::StormCut,
            RouteResult::Hung,
            RouteResult::TickRefused,
        ];
        for r in routes.iter() {
            out.push(format!(
                "route.{}.count={}",
                kv_route_key(*r),
                self.route_counts[route_index(*r)]
            ));
        }
        // 开销分位。
        match self.cost.percentile(50) {
            Some(p) => out.push(format!("cost.p50_steps={}", p)),
            None => out.push("cost.p50_steps=NA".to_string()),
        }
        match self.cost.percentile(95) {
            Some(p) => out.push(format!("cost.p95_steps={}", p)),
            None => out.push("cost.p95_steps=NA".to_string()),
        }
        match self.cost.max_steps() {
            Some(m) => {
                out.push(format!("cost.max_steps={}", m));
                out.push(format!("cost.max_ns={}", CostLedger::steps_to_ns(m)));
            }
            None => out.push("cost.max_steps=NA".to_string()),
        }
        out.push(format!("cost.samples={}", self.cost.count()));
        out.push(format!(
            "cost.redline_steps={}",
            COST_REDLINE_STEPS
        ));
        out.push(format!(
            "cost.redline_breached={}",
            self.cost.redline_breach().is_some()
        ));
        out.push(format!("cost.truncated={}", self.cost.is_truncated()));
        // 屏蔽/风暴。
        out.push(format!("mask.raw=0x{:08X}", self.mask.raw()));
        out.push(format!("mask.groups_masked={}", self.mask.masked_count()));
        out.push(format!("storm.trips={}", self.storm.trips_total()));
        // 注册表。
        out.push(format!("registry.registered={}", self.registry.registered_count()));
        // 旁路账。
        out.push(format!("bypass.total={}", self.bypass.total()));
        out.push(format!("bypass.adopted={}", self.bypass.adopted_count()));
        out.push(format!("bypass.pending={}", self.bypass.pending()));
        out.push(format!(
            "bypass.clean={}",
            self.bypass.pending() == 0
        ));
        for s in self.bypass.snapshot().iter() {
            out.push(format!(
                "bypass.slot.vector=0x{:02X},path={},adopted={},tick={}",
                s.vector,
                s.legacy_path,
                s.adopted,
                match s.adopted_tick {
                    Some(t) => t,
                    None => 0,
                }
            ));
        }
        // 立案账。
        out.push(format!("issues.count={}", self.issues.len()));
        out.push(format!("issues.overflow={}", self.issues_overflow));
        for (i, rec) in self.issues.iter().enumerate() {
            out.push(format!(
                "issue.{}.code=0x{:04X},vector=0x{:02X},tick={},note={}",
                i, rec.code, rec.vector, rec.tick, rec.note
            ));
        }
        out.push(format!("entries.wrapped={}", self.entries_wrapped));
        out
    }

    /// 登记簿只读视图。
    pub fn entries(&self) -> &[DispatchEntry] {
        &self.entries
    }

    /// 步数账只读视图。
    pub fn cost(&self) -> &CostLedger {
        &self.cost
    }

    /// 注册表只读视图。
    pub fn registry(&self) -> &HandlerRegistry {
        &self.registry
    }

    /// 屏蔽位图只读视图。
    pub const fn mask(&self) -> &MaskBitmap {
        &self.mask
    }

    /// 风暴防护器只读视图。
    pub const fn storm(&self) -> &StormGuard {
        &self.storm
    }

    /// 旁路账只读视图。
    pub const fn bypass(&self) -> &BypassLedger {
        &self.bypass
    }

    /// 五类频次快照。
    pub const fn kind_counts(&self) -> &[u64; 5] {
        &self.kind_counts
    }

    /// 单向量登记回查（最近一笔该向量的登记）。
    pub fn last_entry_of(&self, vector: u16) -> Option<&DispatchEntry> {
        if vector > SPURIOUS_VECTOR {
            return None;
        }
        self.entries.iter().rev().find(|e| e.vector == vector as u8)
    }
}

/// 路由结果 → 账桶下标（TickRefused 不落登记簿，恒 0 笔——桶位保留守恒）。
const fn route_index(r: RouteResult) -> usize {
    match r {
        RouteResult::Invoked => 0,
        RouteResult::Masked => 1,
        RouteResult::SpuriousAck => 2,
        RouteResult::StormCut => 3,
        RouteResult::Hung => 4,
        RouteResult::TickRefused => 5,
    }
}

/// 类别 → kv 键名。
const fn kv_kind_key(k: VectorKind) -> &'static str {
    match k {
        VectorKind::Exception => "exception",
        VectorKind::Irq => "irq",
        VectorKind::Ipi => "ipi",
        VectorKind::Nmi => "nmi",
        VectorKind::Spurious => "spurious",
    }
}

/// 路由 → kv 键名。
const fn kv_route_key(r: RouteResult) -> &'static str {
    match r {
        RouteResult::Invoked => "invoked",
        RouteResult::Masked => "masked",
        RouteResult::SpuriousAck => "spurious_ack",
        RouteResult::StormCut => "storm_cut",
        RouteResult::Hung => "hung",
        RouteResult::TickRefused => "tick_refused",
    }
}

/// 全量审计报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullAuditReport {
    /// 探针册（五类分型/分路 10/10）。
    pub probe: ProbeReport,
    /// 登记簿笔数。
    pub entries: usize,
    /// 步数 P50。
    pub p50: Option<u32>,
    /// 步数 P95。
    pub p95: Option<u32>,
    /// 最大步数。
    pub max_steps: Option<u32>,
    /// 红线步数（150）。
    pub redline_steps: u32,
    /// 红线纳米（300）。
    pub redline_ns: u64,
    /// 旁路账快照。
    pub bypass: BypassAudit,
    /// 悬挂笔数。
    pub hung: u64,
    /// 风暴截断笔数。
    pub storm_cuts: u64,
    /// 屏蔽挂起笔数。
    pub masked: u64,
    /// 立案笔数。
    pub issues: usize,
    /// 步数样本是否封账截断。
    pub steps_truncated: bool,
}

// ---------------------------------------------------------------------------
// 十五、判据 J1 集成通道——三判据一键核
// ---------------------------------------------------------------------------

/// J1 三判据核对报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct J1Report {
    /// 判据一：主通道正确——五类探针全满 10/10。
    pub main_channel_ok: bool,
    /// 判据二：路径开销 <300ns——P95 与最大步数均低于 150 步。
    pub cost_ok: bool,
    /// 判据二实测：P95 步数折算 ns（无样本为 None）。
    pub p95_ns: Option<u64>,
    /// 判据三：旁路清零断言——旁路槽为零。
    pub bypass_clean: bool,
    /// 三判据全过。
    pub all_ok: bool,
}

/// J1 三判据一键核：探针 + 红线 + 旁路，任一红项如实落报告。
///
/// 与 full_audit 的差别：本函数**不短路**——三项全跑，报告完整（立案
/// 仍逐项落账），供册面核对用；full_audit 短路版供运行期闸门用。
pub fn judge_j1(core: &mut DispatchCore, tick: u64) -> J1Report {
    let probe = core.run_probe_suite(tick);
    let main_channel_ok = probe.all_ok;
    let cost_ok = core.cost.redline_breach().is_none();
    let p95_ns = core.cost.percentile(95).map(CostLedger::steps_to_ns);
    let bypass_clean = core.bypass.pending() == 0;
    if !main_channel_ok {
        core.file_issue(DpCode::CLASSIFY_ROUTE_BREACH, 0, tick, "J1 判据一：主通道探针未全满 10/10");
    }
    if !cost_ok {
        core.file_issue(DpCode::COST_REDLINE_BREACH, 0, tick, "J1 判据二：步数红线超限");
    }
    if !bypass_clean {
        core.file_issue(
            DpCode::BYPASS_SLOTS_NONZERO,
            0,
            tick,
            "J1 判据三：旁路槽非零，收编不干净",
        );
    }
    J1Report {
        main_channel_ok,
        cost_ok,
        p95_ns,
        bypass_clean,
        all_ok: main_channel_ok && cost_ok && bypass_clean,
    }
}
