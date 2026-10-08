//! VE-F0219 · virtio 主客联调通道（VE-B 域 · 宿主↔客户调试消息对通 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0219`
//!
//! **职责定位（锚点原文）**：宿主侧与客户侧调试消息对通——宿主可注入调试命令
//! （开启逐帧日志、强制刷新、快照导出），客户侧事件可回传；通道**独立于渲染
//! 数据面**且带宽有配额，防调试流量干扰渲染；命令非法拒绝并回执说明。
//!
//! **判据（锚点原文）**：**独立通道、配额限流、回执语义、协议复用、判据**。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//!
//! - 通道不通 → **降级仅宿主侧调试并告知**（不是全丢：宿主本地日志仍可用）
//! - 命令非法 → **拒绝并回执原因**（拒绝必须带原因，否则调用方无从纠正）
//! - 带宽越限 → **限流但安全类消息不丢弃**（安全类不能被限流吞掉）
//!
//! **数据结构**：调试消息（类型 × 参数 × 回执）[`DebugMsg` / `Receipt`]；
//! 通道配额记录 [`QuotaRecord`]。
//!
//! **性能逐项分解（锚点原文）**：消息 O(1)；配额计数 O(1)；
//! **渲染数据面零感知**——本通道不碰任何渲染状态，故渲染路径上零指令。
//!
//! **跨批对接点**：上游 F0201 至 F0214；下游 **F0239 Intel 调试通道沿用同
//! 协议骨架**。故 [`MsgKind::wire`] 是**显式映射**而非 `enum as u8`：
//! F0239 要在这套骨架上换一批线上编码，判别值不能被绑死。
//!
//! ## 设计要点
//!
//! - **安全类消息不可被限流丢弃**（[`MsgKind::is_safety`]）：限流器对
//!   [`MsgKind::Safety`] 类**直接放行**，不占配额也不计丢弃。这不是"特权"，
//!   是因为安全类消息（看门狗喂狗、致命错误上报）一旦被丢，故障就再也
//!   查不出来——**丢掉的是最后的证据**。
//! - **配额用「字节当量」而非「条数」**（[`MsgKind::cost`]）：一条快照导出
//!   与一条开关日志的带宽差三个数量级，按条限流会让前者挤掉后者。故每类
//!   消息有字节当量，配额按字节计。
//! - **限流必须显式可见**（[`Receipt::is_throttled`] 与
//!   [`QuotaRecord::throttled`]）：被限流丢弃的消息要有回执说明"被限流"，
//!   静默丢弃等于调试通道撒谎。
//! - **拒绝必须带原因码**（[`Reject`]）：非法命令回执写明是
//!   未知类型/参数个数/参数值/参数语义，调用方才能纠正。
//! - **通道不通时降级为宿主侧**（[`ChannelState::HostOnly`]）：不是全丢，
//!   宿主本地调试仍工作，且回执**告知已降级**——否则用户以为命令生效了。
//!   降级只改「送到哪一侧」，**不改这条命令合不合法**，故参数校验排在
//!   降级**之前**（[`DebugChannel::submit`]）——排在之后的话非法命令会在
//!   宿主侧被真的执行（副作用照发生、账本还记「已处理」）。
//! - **两种降级态不等价**（判据 9 与 10 钉住这个区别）：
//!   [`ChannelState::Down`] 压根没走通道，**不占通道配额**；
//!   [`ChannelState::HostOnly`] 走上半段通道，只落宿主侧，**照占配额**。
//!   配额是通道带宽预算，没走通道就没有带宽可占；而两者都记 `handled`
//!   （全程账本：命令确实在宿主侧执行了）。
//! - **判据侧独立重算**（[`ref_cost`] / [`ref_validate_of`] /
//!   [`ref_semantics_of`]）：判据不调被测函数算期望值，避免自证式恒真。
//!   校验重算**按实现的两阶段拆开**（值域 / 语义），合成一个函数就丢了
//!   阶段信息——「值合法但语义非法」这类判断在合成函数里表达不出来。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F0217（`veb17_suspend`）管设备态快照与恢复**，本条管**调试消息的
//!   传输与配额**。F0217 的"快照导出"在本条是**一条命令**（本条负责发它），
//!   而 F0217 负责快照本身的正确性与可恢复性。
//! - **F0215（`veb15_suite`）管测试套件的三层用例**，本条被它当作执行通道
//!   的一部分（日志开关要真的把日志打开，套件才知道该不该记诊断）。
//!
//! 两条各自**自持**定义类型，不跨模块 `use`——并行提交时跨模块引用会把两个
//! 模块的编译成败绑在一起，一方半成品就拖垮另一方，而这类失败报 E0583，
//! 与真实缺陷长得一样、极难分辨。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 单条消息的字节当量（普通类）：命令头 + 参数 + 回执的最小骨架。
pub const COST_NORMAL: u32 = 32;

/// 快照导出：一次导出的字节当量（比普通消息贵两个数量级）。
pub const COST_SNAPSHOT: u32 = 4096;

/// 强制刷新：一次刷新的字节当量。
pub const COST_FLUSH: u32 = 256;

/// 逐帧日志开关：仅一个控制位，字节当量最小。
pub const COST_LOG_TOGGLE: u32 = 8;

/// 安全类消息（看门狗/致命错误上报）的字节当量。
pub const COST_SAFETY: u32 = 16;

/// 通道单周期带宽配额（字节当量）。
pub const QUOTA_PERIOD_BYTES: u32 = 64 * 1024;

/// 消息参数个数上限（超出即参数越界）。
pub const MAX_PARAMS: usize = 8;

/// 参数值上限（`u32` 域内的上界，超出即越界）。
pub const MAX_PARAM_VALUE: u32 = 1 << 20;

/// 回执历史环形缓冲深度（保留最近若干条以便回看）。
pub const RECEIPT_RING: usize = 16;

/// 消息类型种数（由 [`MsgKind::ALL`] 的长度约束，不是随意写的数）。
pub const MSG_KIND_COUNT: usize = 5;

/// 拒绝原因种数。
pub const REJECT_COUNT: usize = 4;

/// 通道状态种数。
pub const STATE_COUNT: usize = 3;

// ---------------------------------------------------------------------------
// 二、消息类型与拒绝原因
// ---------------------------------------------------------------------------

/// 调试消息类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsgKind {
    /// 开关逐帧日志。
    LogToggle,
    /// 强制刷新（管线冲刷）。
    Flush,
    /// 导出快照。
    SnapshotExport,
    /// 客户侧事件回传。
    GuestEvent,
    /// **安全类**：看门狗喂狗与致命错误上报。
    Safety,
}

impl MsgKind {
    /// 全集，顺序稳定（判据按此下标推导，不靠字面量）。
    pub const ALL: [MsgKind; MSG_KIND_COUNT] = [
        MsgKind::LogToggle,
        MsgKind::Flush,
        MsgKind::SnapshotExport,
        MsgKind::GuestEvent,
        MsgKind::Safety,
    ];

    /// 判别下标（等于在 [`MsgKind::ALL`] 中的位置）。
    pub const fn ordinal(self) -> usize {
        match self {
            MsgKind::LogToggle => 0,
            MsgKind::Flush => 1,
            MsgKind::SnapshotExport => 2,
            MsgKind::GuestEvent => 3,
            MsgKind::Safety => 4,
        }
    }

    /// **线上编码值**（F0239 Intel 通道沿用同骨架、换一批值）。
    ///
    /// 刻意**不用** `enum as u8`：判别值是源码顺序，线上编码是对外契约，
    /// 两者绑死则重排枚举就静默改了协议——这类事故极难查。
    pub const fn wire(self) -> u16 {
        match self {
            MsgKind::LogToggle => 0x1001,
            MsgKind::Flush => 0x1002,
            MsgKind::SnapshotExport => 0x1003,
            MsgKind::GuestEvent => 0x1004,
            MsgKind::Safety => 0x1F01,
        }
    }

    /// 由线上编码反查类型（未知编码返回 [`None`]）。
    pub const fn from_wire(w: u16) -> Option<MsgKind> {
        match w {
            0x1001 => Some(MsgKind::LogToggle),
            0x1002 => Some(MsgKind::Flush),
            0x1003 => Some(MsgKind::SnapshotExport),
            0x1004 => Some(MsgKind::GuestEvent),
            0x1F01 => Some(MsgKind::Safety),
            _ => None,
        }
    }

    /// 字节当量（配额按**字节**计，不按条数）。
    pub const fn cost(self) -> u32 {
        match self {
            MsgKind::LogToggle => COST_LOG_TOGGLE,
            MsgKind::Flush => COST_FLUSH,
            MsgKind::SnapshotExport => COST_SNAPSHOT,
            MsgKind::GuestEvent => COST_NORMAL,
            MsgKind::Safety => COST_SAFETY,
        }
    }

    /// 是否**安全类**（限流时**不丢弃**）。
    pub const fn is_safety(self) -> bool {
        matches!(self, MsgKind::Safety)
    }

    /// 该类型要求的参数个数。
    ///
    /// **不是所有类型都要参数**：刷新与安全上报是**零参数**命令，
    /// 多给一个反而是调用方搞错了位。
    ///
    /// 定义在 [`MsgKind`] 上而非 [`DebugMsg`]：这是**类型的固有属性**，
    /// 与某条消息带了什么参数无关——同类型的所有消息要求都相同。
    pub const fn wanted_params(self) -> usize {
        match self {
            MsgKind::LogToggle => 1, // 一个开关位
            MsgKind::Flush => 0,
            MsgKind::SnapshotExport => 0,
            MsgKind::GuestEvent => 1, // 一个事件码
            MsgKind::Safety => 0,
        }
    }

    /// 中文标签。
    pub const fn zh(self) -> &'static str {
        match self {
            MsgKind::LogToggle => "逐帧日志开关",
            MsgKind::Flush => "强制刷新",
            MsgKind::SnapshotExport => "快照导出",
            MsgKind::GuestEvent => "客户侧事件回传",
            MsgKind::Safety => "安全类上报",
        }
    }

    /// 英文标签（协议名，供 F0239 复用时对齐）。
    pub const fn tag(self) -> &'static str {
        match self {
            MsgKind::LogToggle => "log-toggle",
            MsgKind::Flush => "flush",
            MsgKind::SnapshotExport => "snapshot-export",
            MsgKind::GuestEvent => "guest-event",
            MsgKind::Safety => "safety",
        }
    }
}

/// 判别值 → 类型（越界返回 [`None`]）。
pub const fn kind_by_ordinal(i: usize) -> Option<MsgKind> {
    if i < MSG_KIND_COUNT {
        Some(MsgKind::ALL[i])
    } else {
        None
    }
}

/// 线上编码 → 类型（协议入口）。
pub const fn wire_kind(w: u16) -> Option<MsgKind> {
    MsgKind::from_wire(w)
}

/// 拒绝原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    /// 参数个数超出 [`MAX_PARAMS`]。
    ParamCount,
    /// 参数值越界（超出 [`MAX_PARAM_VALUE`]）。
    ParamValue,
    /// 参数个数与该类型要求的语义不符（如刷新类给了参数）。
    ParamSemantics,
    /// 线上编码不认识（带具体编码，供审计）。
    UnknownKind(u16),
}

impl Reject {
    /// 全集（末项是 [`Reject::UnknownKind`] 的占位载荷）。
    pub const ALL: [Reject; REJECT_COUNT] = [
        Reject::ParamCount,
        Reject::ParamValue,
        Reject::ParamSemantics,
        Reject::UnknownKind(0),
    ];

    /// 判别下标（按 [`Reject::ALL`] 顺序）。
    ///
    /// [`Reject::UnknownKind`] 的下标**与其载荷无关**（无论载荷是哪个
    /// 编码都归到同一类）——否则「不认识 0x9999」与「不认识 0x1001」会被
    /// 算成两类，计数就按攻击面分裂了。
    pub const fn ordinal(self) -> usize {
        match self {
            Reject::ParamCount => 0,
            Reject::ParamValue => 1,
            Reject::ParamSemantics => 2,
            Reject::UnknownKind(_) => 3,
        }
    }

    /// 原因码（线上编码，跨实现可比）。
    pub const fn code(self) -> u8 {
        match self {
            Reject::ParamCount => 1,
            Reject::ParamValue => 2,
            Reject::ParamSemantics => 3,
            Reject::UnknownKind(_) => 4,
        }
    }

    /// 中文说明（回执必须带原因，调用方才能纠正）。
    pub const fn zh(self) -> &'static str {
        match self {
            Reject::ParamCount => "参数个数越界",
            Reject::ParamValue => "参数值越界",
            Reject::ParamSemantics => "参数语义不符",
            Reject::UnknownKind(_) => "未知消息类型",
        }
    }

    /// 是否携带了具体的未知编码（供审计留痕用）。
    pub const fn has_payload(self) -> bool {
        matches!(self, Reject::UnknownKind(_))
    }

    /// 载荷（仅 [`Reject::UnknownKind`] 有，其余为 0）。
    pub const fn payload(self) -> u16 {
        match self {
            Reject::UnknownKind(w) => w,
            _ => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、消息与回执
// ---------------------------------------------------------------------------

/// 一条调试消息（类型 × 参数）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugMsg {
    /// 消息类型（由线上编码解出）。
    pub kind: MsgKind,
    /// 原始线上编码（保留，供审计回放）。
    pub raw_wire: u16,
    /// 参数。
    pub params: Vec<u32>,
}

impl DebugMsg {
    /// 按类型构造一条消息（`raw_wire` 取该类型的规范编码）。
    pub fn new(kind: MsgKind, params: Vec<u32>) -> DebugMsg {
        DebugMsg { kind, raw_wire: kind.wire(), params }
    }

    /// 从线上编码构造（**解码入口**，F0239 复用此入口）。
    ///
    /// 返回 `Err(Reject::UnknownKind(w))` 而非静默丢弃：协议入口丢掉
    /// 不认识的编码，调用方会以为命令生效了。
    pub fn from_wire(w: u16, params: Vec<u32>) -> Result<DebugMsg, Reject> {
        match MsgKind::from_wire(w) {
            Some(k) => Ok(DebugMsg { kind: k, raw_wire: w, params }),
            None => Err(Reject::UnknownKind(w)),
        }
    }

    /// 参数校验（**不含**类型语义检查，那在通道层做）。
    ///
    /// 校验顺序刻意为「个数 → 值」：个数不对时逐个查值毫无意义，
    /// 且报错顺序要与 [`Reject::ordinal`] 一致，便于回执按码归类。
    pub fn validate(&self) -> Result<(), Reject> {
        if self.params.len() > MAX_PARAMS {
            return Err(Reject::ParamCount);
        }
        let mut i = 0usize;
        while i < self.params.len() {
            if self.params[i] > MAX_PARAM_VALUE {
                return Err(Reject::ParamValue);
            }
            i += 1;
        }
        Ok(())
    }

    /// 类型语义校验：参数个数须**恰好等于**要求，且布尔位只能是 0/1。
    pub fn check_semantics(&self) -> Result<(), Reject> {
        if self.params.len() != self.kind.wanted_params() {
            return Err(Reject::ParamSemantics);
        }
        // 开关位与事件码是布尔语义，其它值是调用方误传。
        if self.params.len() == 1 && self.params[0] > 1 {
            return Err(Reject::ParamValue);
        }
        Ok(())
    }

    /// 字节当量（转发判据侧的独立重算，避免两处公式各写一遍走偏）。
    pub fn bytes(&self) -> u32 {
        ref_cost(self.kind, self.params.len())
    }

    /// 一行审计摘要。
    pub fn summary(&self) -> String {
        format!(
            "{} / {}：{} 个参数",
            self.kind.zh(),
            self.kind.tag(),
            self.params.len()
        )
    }
}

/// 回执（宿主必回一条：接受 / 拒绝 / 限流 / 降级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Receipt {
    /// 已接受并执行。
    Accepted,
    /// 拒绝（带原因）。
    Rejected(Reject),
    /// 被限流丢弃（带类型，**仅非安全类**会出现）。
    Throttled(MsgKind),
    /// 已接受，但通道不通、只做了宿主侧降级处理。
    DegradedToHost,
}

impl Receipt {
    /// 是否**成功**（真正送到对端执行了）。
    pub const fn is_ok(self) -> bool {
        matches!(self, Receipt::Accepted | Receipt::DegradedToHost)
    }

    /// 是否**被限流丢弃**（安全类**绝不会**走到这里——这是判据要钉的性质）。
    pub const fn is_throttled(self) -> bool {
        matches!(self, Receipt::Throttled(_))
    }

    /// 是否**被拒绝**。
    pub const fn is_rejected(self) -> bool {
        matches!(self, Receipt::Rejected(_))
    }

    /// 拒绝原因（**仅** [`Receipt::Rejected`] 有，其余给 [`None`]——
    /// 不能给占位值，那会让调用方在没判 `is_rejected()` 时误读一个原因）。
    pub const fn reason(self) -> Option<Reject> {
        match self {
            Receipt::Rejected(r) => Some(r),
            _ => None,
        }
    }

    /// 回执的一行中文说明（**拒绝必带原因**，锚点硬要求）。
    pub fn line(self) -> String {
        match self {
            Receipt::Accepted => format!("已接受并执行"),
            Receipt::DegradedToHost => {
                format!("通道不通：已降级为仅宿主侧调试并告知")
            }
            Receipt::Rejected(r) => format!("已拒绝：{}（码 {}）", r.zh(), r.code()),
            Receipt::Throttled(k) => {
                format!("已被限流丢弃：{}（安全类消息不丢弃）", k.zh())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 四、通道状态与配额
// ---------------------------------------------------------------------------

/// 通道状态。
///
/// `Default` 取 [`ChannelState::Up`]（新建即通畅）：默认若是 `Down`，
/// 忘了调 `set_state` 的通道会**静默吞掉所有命令**——降级路径比通畅路径
/// 更容易被当成「正常」，那是最坏的默认值。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChannelState {
    /// 双向通畅。
    #[default]
    Up,
    /// 双向不通。
    Down,
    /// 只能宿主侧（降级态；宿主本地调试仍可用）。
    HostOnly,
}

impl ChannelState {
    /// 全集（判据按此下标推导）。
    pub const ALL: [ChannelState; STATE_COUNT] =
        [ChannelState::Up, ChannelState::Down, ChannelState::HostOnly];

    /// 判别下标。
    pub const fn ordinal(self) -> usize {
        match self {
            ChannelState::Up => 0,
            ChannelState::Down => 1,
            ChannelState::HostOnly => 2,
        }
    }

    /// 中文标签。
    pub const fn zh(self) -> &'static str {
        match self {
            ChannelState::Up => "双向通畅",
            ChannelState::Down => "双向不通",
            ChannelState::HostOnly => "仅宿主侧（已降级）",
        }
    }

    /// 是否**对客可达**（[`ChannelState::HostOnly`] 不可用——对客那条断了）。
    pub const fn guest_reachable(self) -> bool {
        matches!(self, ChannelState::Up)
    }
}

/// 配额记录（**限流必须显式可见**：限流数与安全旁路数都是可查的）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QuotaRecord {
    /// 本周期已用字节（安全类**不计入**：它不受配额约束）。
    pub used_bytes: u32,
    /// 本周期被限流丢弃的条数（安全类不计入此处）。
    pub throttled: u32,
    /// 本周期安全类放行条数（**单独计数**，否则限流器一改就看不出安全类走过）。
    pub safety_bypassed: u32,
    /// 本周期已接受条数。
    pub accepted: u32,
}

impl QuotaRecord {
    /// 剩余配额字节（饱和减，不回绕）。
    pub fn remaining(&self) -> u32 {
        QUOTA_PERIOD_BYTES.saturating_sub(self.used_bytes)
    }

    /// 是否还够放一条 `bytes` 字节的消息。
    ///
    /// **用 `<=` 不是 `<`**：剩余恰好等于消息字节时**放行**（配额是
    /// 「不超过」语义）。写成 `<` 会让「正好用满」这条边界被误拒，
    /// 而那是最该放行的一种。
    pub fn fits(&self, bytes: u32) -> bool {
        bytes <= self.remaining()
    }

    /// 限流器判定：**安全类直接放行**（不占配额也不计丢弃）。
    pub fn admit(&mut self, kind: MsgKind, bytes: u32) -> bool {
        if kind.is_safety() {
            self.safety_bypassed = self.safety_bypassed.saturating_add(1);
            return true;
        }
        if self.fits(bytes) {
            self.used_bytes = self.used_bytes.saturating_add(bytes);
            true
        } else {
            self.throttled = self.throttled.saturating_add(1);
            false
        }
    }
}

// ---------------------------------------------------------------------------
// 五、调试通道
// ---------------------------------------------------------------------------

/// 宿主↔客户调试通道。
///
/// **独立于渲染数据面**：本结构不持有任何渲染状态引用，渲染路径上
/// 因此**零指令**——锚点要求「渲染数据面零感知」，靠的是结构上不碰。
#[derive(Clone, Debug, Default)]
pub struct DebugChannel {
    /// 通道状态。
    pub state: ChannelState,
    /// 配额记录。
    pub quota: QuotaRecord,
    /// 已接受的消息类型计数（按 [`MsgKind::ALL`] 下标，绝对值口径）。
    pub handled: [u32; MSG_KIND_COUNT],
    /// 各拒绝原因计数（按 [`Reject::ALL`] 下标）。
    pub rejects: [u32; REJECT_COUNT],
    /// 回执环形缓冲（保留最近 [`RECEIPT_RING`] 条）。
    pub receipts: Vec<Receipt>,
    /// 逐帧日志是否开着（由 [`MsgKind::LogToggle`] 命令驱动）。
    pub frame_log: bool,
}

impl DebugChannel {
    /// 新建通道（初始为双向通畅）。
    pub fn new() -> DebugChannel {
        DebugChannel { state: ChannelState::Up, ..Default::default() }
    }

    /// 置通道状态。
    pub fn set_state(&mut self, s: ChannelState) {
        self.state = s;
    }

    /// 周期切换：清零**周期间**配额（已用字节、限流数、安全旁路数、接受数）。
    ///
    /// 只清配额不清各类型累计计数——后者是全程账本，前者是周期间账本，
    /// 混在一起就看不出「哪个周期被限流过」。
    pub fn roll_period(&mut self) {
        self.quota = QuotaRecord::default();
    }

    /// 送一条消息进通道，拿回执。
    ///
    /// 次序刻意为 **校验 → 降级 → 限流(安全旁路) → 状态分支**：
    /// 1. 先校参数（个数 → 值 → 语义），非法即拒绝并回执原因。
    ///    **校验必须排在降级之前**：降级只决定「这条命令作用在哪一侧」，
    ///    不改变「这条命令合不合法」。若把降级放在最前，非法命令会绕过
    ///    校验被**真的执行**——`LogToggle` 的开关位被写成非法值也会
    ///    落到 `frame_log` 上，且 `handled` 照记，等于账本里混进了
    ///    一条「已处理的非法命令」，事后无法分辨。
    /// 2. 再看通道是否**完全不通**——不通则降级为仅宿主侧（不是丢弃：
    ///    宿主本地调试仍可用），回执**告知已降级**。此路径**不占配额**：
    ///    压根没走通道就没有通道带宽可占，但 `handled` 照记（命令确实
    ///    在宿主侧执行了，这是全程账本）。
    /// 3. 再过配额，**安全类在此旁路**（不受配额约束）。安全类**不能**
    ///    排到第 2 步之前——通道真不通时它同样送不出，那时才该降级；
    ///    但通道通的情况下它必须抢在配额检查**之前**走独立分支，
    ///    否则会被连带限流丢掉，而安全类被丢等于丢掉最后的证据。
    pub fn submit(&mut self, msg: &DebugMsg) -> Receipt {
        // 1) 参数与语义校验：非法即拒绝并回执原因（**与通道状态无关**）
        if let Err(r) = msg.validate() {
            return self.note_reject(r);
        }
        if let Err(r) = msg.check_semantics() {
            return self.note_reject(r);
        }

        // 2) 通道完全不通 ⇒ 降级为仅宿主侧（不占配额，但仍执行本地副作用）
        if self.state == ChannelState::Down {
            self.apply(msg);
            let rc = Receipt::DegradedToHost;
            self.record(rc);
            return rc;
        }

        // 3) 配额限流：**安全类不丢弃**（限流器内判定）
        if !self.quota.admit(msg.kind, msg.bytes()) {
            let rc = Receipt::Throttled(msg.kind);
            self.record(rc);
            return rc;
        }

        // 4) 仅宿主侧降级态：对客那条已断，命令只作用于宿主
        let rc = if self.state == ChannelState::Up {
            Receipt::Accepted
        } else {
            Receipt::DegradedToHost
        };
        self.apply(msg);
        self.record(rc);
        rc
    }

    /// 执行命令的本地效果。
    ///
    /// 日志开关**真的会改** `frame_log`——否则「开启了逐帧日志」只是句空话，
    /// 依赖它的套件无法据此决定是否记诊断。
    fn apply(&mut self, msg: &DebugMsg) {
        let i = msg.kind.ordinal();
        self.handled[i] = self.handled[i].saturating_add(1);
        self.quota.accepted = self.quota.accepted.saturating_add(1);
        if msg.kind == MsgKind::LogToggle && msg.params.len() == 1 {
            self.frame_log = msg.params[0] == 1;
        }
    }

    /// 记一条回执（环形缓冲，深度 [`RECEIPT_RING`]）。
    pub fn record(&mut self, r: Receipt) {
        if self.receipts.len() >= RECEIPT_RING {
            self.receipts.remove(0);
        }
        self.receipts.push(r);
    }

    /// 某类消息的已接受计数。
    pub fn handled_of(&self, k: MsgKind) -> u32 {
        self.handled[k.ordinal()]
    }

    /// 某拒绝原因的计数。
    pub fn reject_of(&self, r: Reject) -> u32 {
        self.rejects[r.ordinal()]
    }

    /// 记一次拒绝并生成回执（拒绝路径统一走这里，保证计数与留痕一致）。
    fn note_reject(&mut self, r: Reject) -> Receipt {
        let i = r.ordinal();
        self.rejects[i] = self.rejects[i].saturating_add(1);
        let rc = Receipt::Rejected(r);
        self.record(rc);
        rc
    }

    /// 读屏面板：七行中英双语。
    ///
    /// **只报聚合计数**，不报任何用户内容——锚点写明「调试输出不含用户
    /// 内容（经 F0097）」，故面板里既没有消息参数值也没有回执原文。
    pub fn a11y_lines(&self) -> [String; 7] {
        [
            format!("通道状态 / channel state: {}", self.state.zh()),
            format!("已接受 / accepted: {}", self.quota.accepted),
            format!("限流丢弃 / throttled: {}", self.quota.throttled),
            format!("安全类放行 / safety bypassed: {}", self.quota.safety_bypassed),
            format!("已用配额字节 / quota used: {}", self.quota.used_bytes),
            format!("配额剩余字节 / quota remaining: {}", self.quota.remaining()),
            format!("回执条数 / receipts: {}", self.receipts.len()),
        ]
    }
}

// ---------------------------------------------------------------------------
// 六、判据侧独立重算（**不调被测对象**，避免自证式恒真）
// ---------------------------------------------------------------------------

/// 判据侧独立重算的消息字节当量（与 [`DebugMsg::bytes`] 解耦）。
///
/// 刻意**另写一份公式**：判据问被测函数要答案就是自证式（问它要什么
/// 它当然给什么）。
pub fn ref_cost(kind: MsgKind, params: usize) -> u32 {
    let base = match kind {
        MsgKind::LogToggle => COST_LOG_TOGGLE,
        MsgKind::Flush => COST_FLUSH,
        MsgKind::SnapshotExport => COST_SNAPSHOT,
        MsgKind::GuestEvent => COST_NORMAL,
        MsgKind::Safety => COST_SAFETY,
    };
    base.saturating_add((params as u32).saturating_mul(4))
}

/// 判据侧独立重算的**值域阶段**拒绝（个数 + 参数值，对应 [`DebugMsg::validate`]）。
///
/// 刻意与 [`ref_semantics_of`] **拆开**：两阶段各有各的阈值，合成一个
/// 函数就表达不出「值合法但语义非法」——`LogToggle` 传
/// [`MAX_PARAM_VALUE`] 时值域阶段合法、语义阶段非法，合成后只看得到
/// 一个 `Some(ParamValue)`，分不清是哪道门拒的；而判据要钉的恰恰是
/// 「这两件事不能混同」。
pub fn ref_validate_of(params: &[u32]) -> Option<Reject> {
    if params.len() > MAX_PARAMS {
        return Some(Reject::ParamCount);
    }
    let mut i = 0usize;
    while i < params.len() {
        if params[i] > MAX_PARAM_VALUE {
            return Some(Reject::ParamValue);
        }
        i += 1;
    }
    None
}

/// 判据侧独立重算的**语义阶段**拒绝（参数个数须恰好 + 布尔位，对应
/// [`DebugMsg::check_semantics`]）。
pub fn ref_semantics_of(kind: Option<MsgKind>, params: &[u32]) -> Option<Reject> {
    let k = match kind {
        Some(k) => k,
        None => return Some(Reject::UnknownKind(0)),
    };
    let want = match k {
        MsgKind::LogToggle => 1usize,
        MsgKind::Flush => 0,
        MsgKind::SnapshotExport => 0,
        MsgKind::GuestEvent => 1,
        MsgKind::Safety => 0,
    };
    if params.len() != want {
        return Some(Reject::ParamSemantics);
    }
    if params.len() == 1 && params[0] > 1 {
        return Some(Reject::ParamValue);
    }
    None
}

/// 协议骨架复用面（F0239 Intel 通道沿用同一套抽象）。
///
/// 判据钉的是**抽象面**：五个族方法在两个实现之间**逐项同名同语义**，
/// 换实现只需再写一套 `MsgKind` + `Receipt`，通道骨架原样可用。
pub fn protocol_surface() -> [(&'static str, &'static str); 5] {
    [
        ("MsgKind", "类型族：线上编码显式映射（不用 enum as u8）+ 反查 + 字节当量 + 安全类标记"),
        ("DebugMsg", "消息体：类型 × 参数，解码入口 from_wire 返回 Err 而非静默丢弃"),
        ("Receipt", "回执族：接受 / 拒绝带原因码 / 限流带类型 / 降级告知"),
        ("QuotaRecord", "配额记录：字节当量计费 + 安全类旁路且单独计数"),
        ("DebugChannel", "通道：submit 编排 降级→校验→限流(安全旁路)→状态分支"),
    ]
}

// ---------------------------------------------------------------------------
// 七、判据
// ---------------------------------------------------------------------------

/// VE-F0219 模块自检（受 `CheckSet::MAX_CHECKS=112` 约束，逐条覆盖锚点判据）。
pub fn run_veb19_checks() -> CheckSet {
    let mut s = CheckSet::new("veb19_debugchan");

    // --- 判据 1：协议编码显式映射且往返自洽（不用 enum as u8）----------------
    {
        // 逐条走 ALL：wire 互异、from_wire 往返回到自己、ordinal 与 ALL 位置一致。
        let mut wires: [u16; MSG_KIND_COUNT] = [0; MSG_KIND_COUNT];
        let mut ok = true;
        let mut i = 0usize;
        while i < MSG_KIND_COUNT {
            let k = MsgKind::ALL[i];
            wires[i] = k.wire();
            // 往返：编码 → 类型 → 编码
            ok &= MsgKind::from_wire(k.wire()) == Some(k);
            ok &= wire_kind(k.wire()) == Some(k);
            // 判别下标与 ALL 位置一致（判据不写死字面量下标）
            ok &= k.ordinal() == i;
            ok &= kind_by_ordinal(i) == Some(k);
            i += 1;
        }
        // 两两互异（协议编码重复会让两个类型互相串台）
        let mut j = 0usize;
        while j < MSG_KIND_COUNT {
            let mut k = j + 1;
            while k < MSG_KIND_COUNT {
                ok &= wires[j] != wires[k];
                k += 1;
            }
            j += 1;
        }
        // 未知编码必须解不出（不能瞎猜一个类型）
        ok &= MsgKind::from_wire(0xFFFF).is_none()
            && wire_kind(0).is_none()
            && kind_by_ordinal(MSG_KIND_COUNT).is_none();
        // 中文与英文标签逐项互异（合并则调用方无从分辨是哪类命令）
        let mut dup = false;
        let mut m = 0usize;
        while m < MSG_KIND_COUNT {
            let mut n = m + 1;
            while n < MSG_KIND_COUNT {
                if MsgKind::ALL[m].zh() == MsgKind::ALL[n].zh()
                    || MsgKind::ALL[m].tag() == MsgKind::ALL[n].tag()
                {
                    dup = true;
                }
                n += 1;
            }
            m += 1;
        }
        s.add(
            "B19-协议-线上编码显式映射且往返自洽",
            ok
                && !dup
                && MsgKind::ALL.len() == MSG_KIND_COUNT
                && MsgKind::from_wire(0x1001) == Some(MsgKind::LogToggle)
                && MsgKind::from_wire(0x1F01) == Some(MsgKind::Safety),
            "五类消息线上编码互异、往返自洽、判别下标与 ALL 位置一致、未知编码解不出、中英标签互异",
        );
    }

    // --- 判据 2：解码入口对未知编码返回 Err 而非静默丢弃 ----------------------
    {
        let ok = DebugMsg::from_wire(MsgKind::Flush.wire(), Vec::new()).is_ok();
        let bad = DebugMsg::from_wire(0xDEAD, Vec::new());
        let (is_err, has_payload, payload) = match bad {
            Err(r) => (true, r.has_payload(), r.payload()),
            _ => (false, false, 0),
        };
        // 解码出来的消息必须带回**原始编码**，审计才回放得动
        let rt = match DebugMsg::from_wire(MsgKind::LogToggle.wire(), alloc::vec![1]) {
            Ok(m) => m,
            Err(_) => DebugMsg::new(MsgKind::LogToggle, Vec::new()),
        };
        s.add(
            "B19-协议-解码入口未知编码返回Err并保留原编码",
            ok
                && is_err
                && has_payload
                && payload == 0xDEAD
                // 载荷不影响归类：两个不同未知编码归到同一类（否则计数按
                // 攻击面分裂，「有多少种坏编码」就变成一个增长维度）
                && Reject::UnknownKind(0x1111).ordinal() == Reject::UnknownKind(0x2222).ordinal()
                && rt.raw_wire == MsgKind::LogToggle.wire(),
            "未知编码解码失败并带具体编码；不同未知编码归同一类；解码成功时保留原始编码供审计回放",
        );
    }

    // --- 判据 3：参数校验三态（个数越界 / 值越界 / 语义不符）各自专属 ---------
    {
        // 判据侧**独立重算**期望原因，再与被测结果比对（不调被测方法取答案）
        let too_many: Vec<u32> = (0..(MAX_PARAMS + 1)).map(|i| i as u32).collect();
        let m_count = DebugMsg::new(MsgKind::Flush, too_many.clone());
        let m_value = DebugMsg::new(MsgKind::LogToggle, alloc::vec![MAX_PARAM_VALUE + 1]);
        let m_sem = DebugMsg::new(MsgKind::Flush, alloc::vec![1]);
        let want_count = ref_validate_of(&too_many);
        let want_value = ref_validate_of(&[MAX_PARAM_VALUE + 1]);
        let want_sem = ref_semantics_of(Some(MsgKind::Flush), &[1]);
        s.add(
            "B19-校验-三态拒绝各有专属原因码",
            m_count.validate() == Err(Reject::ParamCount)
                && m_value.validate() == Err(Reject::ParamValue)
                && m_sem.check_semantics() == Err(Reject::ParamSemantics)
                // 三态**互不相同**：合并则调用方无法针对性纠正
                && Reject::ParamCount.ordinal() != Reject::ParamValue.ordinal()
                && Reject::ParamValue.ordinal() != Reject::ParamSemantics.ordinal()
                && Reject::ParamCount.code() != Reject::ParamValue.code()
                && Reject::ParamSemantics.code() != Reject::ParamValue.code()
                // 判据侧独立重算给出一致的期望
                && want_count == Some(Reject::ParamCount)
                && want_value == Some(Reject::ParamValue)
                && want_sem == Some(Reject::ParamSemantics),
            "参数个数/参数值/参数语义三类错误各有专属原因码与码值；判据侧独立重算一致",
        );
    }

    // --- 判据 4：边界夹逼对钉死参数值阈位置（界前合法/界上/界后一位）---------
    {
        // 值域阈用**多参数**消息夹逼：MAX_PARAM_VALUE 是 20 位，
        // 拿单参数的 LogToggle 测会被布尔语义门（仅 0/1）先拦掉，
        // 测出来的是语义结果而非值域结果——两阶段的门不能互相代答。
        let at = DebugMsg::new(MsgKind::GuestEvent, alloc::vec![MAX_PARAM_VALUE]);
        let over = DebugMsg::new(MsgKind::GuestEvent, alloc::vec![MAX_PARAM_VALUE + 1]);
        // 布尔语义是另一道更严的门：开关位只能是 0/1，
        // 即便它是「合法范围内的值」，传 2 也要被拒
        let bool_bad = DebugMsg::new(MsgKind::LogToggle, alloc::vec![2]);
        s.add(
            "B19-校验-夹逼对钉死参数值阈与布尔位",
            // 界上（恰等于上限）合法、界后一位越界：值域阶段独立重算
            at.validate() == Ok(())
                && over.validate() == Err(Reject::ParamValue)
                && ref_validate_of(&[MAX_PARAM_VALUE]) == None
                && ref_validate_of(&[MAX_PARAM_VALUE + 1]) == Some(Reject::ParamValue)
                // 布尔位 2 被拒（值合法但语义非法——两件事不能混同）：
                // 值域阶段放行、语义阶段拒绝，两阶段结论**必须不同**
                && bool_bad.validate() == Ok(())
                && bool_bad.check_semantics() == Err(Reject::ParamValue)
                && ref_validate_of(&[2]) == None
                && ref_semantics_of(Some(MsgKind::LogToggle), &[2])
                    == Some(Reject::ParamValue)
                // 两阶段对同一输入给出不同结论（合成一个函数就看不出这个）
                && ref_validate_of(&[MAX_PARAM_VALUE]) != ref_semantics_of(
                    Some(MsgKind::LogToggle),
                    &[MAX_PARAM_VALUE],
                )
                // 布尔位 0/1 合法
                && DebugMsg::new(MsgKind::LogToggle, alloc::vec![0])
                    .check_semantics()
                    == Ok(())
                && DebugMsg::new(MsgKind::LogToggle, alloc::vec![1])
                    .check_semantics()
                    == Ok(())
                && ref_semantics_of(Some(MsgKind::LogToggle), &[0]) == None
                && ref_semantics_of(Some(MsgKind::LogToggle), &[1]) == None,
            "参数值上限本身合法、上限+1 越界；布尔位仅 0/1 合法——值合法与语义合法是两件事",
        );
    }

    // --- 判据 5：配额按字节而非按条（贵消息不能被多次小消息挤掉）------------
    {
        let snap = DebugMsg::new(MsgKind::SnapshotExport, Vec::new());
        let log = DebugMsg::new(MsgKind::LogToggle, alloc::vec![1]);
        let mut q = QuotaRecord::default();
        q.admit(snap.kind, snap.bytes());
        let after_snap = q.used_bytes;
        // 判据侧独立重算成本
        let snap_bytes = ref_cost(MsgKind::SnapshotExport, 0);
        let log_bytes = ref_cost(MsgKind::LogToggle, 1);
        s.add(
            "B19-配额-按字节当量计费而非按条数",
            snap_bytes == COST_SNAPSHOT
                && log_bytes == COST_LOG_TOGGLE + 4
                // 快照比日志贵两个数量级以上：按条计费时两者等价
                && snap_bytes > log_bytes * 100
                // 判据侧独立重算与被测一致
                && snap.bytes() == snap_bytes
                && log.bytes() == log_bytes
                // 一条快照吃掉的分额确实很大：整周期配额的 1/16，
                // 即整周期只够放 16 条快照（按条计费时能放 65536 条）
                && after_snap == snap_bytes
                && after_snap * 16 == QUOTA_PERIOD_BYTES
                && snap_bytes * 16 <= QUOTA_PERIOD_BYTES
                && snap_bytes * 17 > QUOTA_PERIOD_BYTES
                // 且配额剩余随之减少（计费真落到了 used_bytes 上）
                && q.remaining() == QUOTA_PERIOD_BYTES.saturating_sub(after_snap),
            "快照导出字节当量比日志开关大两个数量级以上；配额按字节扣减而非按条计数",
        );
    }

    // --- 判据 6：限流但安全类不丢弃（锚点错误路径的核心）--------------------
    {
        let mut c = DebugChannel::new();
        // 把配额灌满：塞快照直到放不下
        let snap = DebugMsg::new(MsgKind::SnapshotExport, Vec::new());
        let mut guard = 0usize;
        while c.quota.fits(snap.bytes()) && guard < 64 {
            c.submit(&snap);
            guard += 1;
        }
        // 前置：配额确实已被灌满
        let filled = !c.quota.fits(snap.bytes()) && c.quota.used_bytes > 0;
        let throttled_before = c.quota.throttled;
        // 非安全类在满配额下被限流
        let r_snap = c.submit(&snap);
        let guest = DebugMsg::new(MsgKind::GuestEvent, alloc::vec![1]);
        let r_guest = c.submit(&guest);
        // 安全类在同样满配额下必须仍被接受。
        // 基线**必须在两条之前取**：取在第一条之后就把它自己算进基线了，
        // 增量恒为 1 而非 2（原来就是这么错的）。
        let safety = DebugMsg::new(MsgKind::Safety, Vec::new());
        let used_before_safe = c.quota.used_bytes;
        let bypass_before = c.quota.safety_bypassed;
        let handled_before_safe = c.handled_of(MsgKind::Safety);
        let r_safe = c.submit(&safety);
        // 再送一条安全类，仍然通行（不是「恰好第一条能过」）
        let r_safe2 = c.submit(&safety);
        s.add(
            "B19-限流-安全类消息永不被限流丢弃",
            filled
                // 非安全类：限流 + 回执可见
                && r_snap.is_throttled()
                && r_guest.is_throttled()
                && c.quota.throttled == throttled_before + 2
                // 安全类：两次都通行，且回执不是 Throttled
                && r_safe == Receipt::Accepted
                && r_safe2 == Receipt::Accepted
                && !r_safe.is_throttled()
                && !r_safe2.is_throttled()
                // 安全类不占配额（否则它自己会把额度吃光）
                && c.quota.used_bytes == used_before_safe
                // 但单独计数，否则改限流器就看不出安全类走过
                && c.quota.safety_bypassed == bypass_before + 2
                // 且真的被执行了（不是「放行但没做事」）
                && c.handled_of(MsgKind::Safety) == handled_before_safe + 2
                // 性质断言，非某一组数据的巧合
                && MsgKind::Safety.is_safety()
                && !MsgKind::SnapshotExport.is_safety(),
            "配额灌满后非安全类被限流且回执可见；安全类连续两条仍通行、不占配额、单独计数、确实被执行",
        );
    }

    // --- 判据 7：限流回执显式可见（不静默丢弃，调试通道不能撒谎）------------
    {
        let mut c = DebugChannel::new();
        let snap = DebugMsg::new(MsgKind::SnapshotExport, Vec::new());
        let mut guard = 0usize;
        while c.quota.fits(snap.bytes()) && guard < 64 {
            c.submit(&snap);
            guard += 1;
        }
        let r = c.submit(&snap);
        let line = r.line();
        let receipts = c.receipts.len();
        // 判据侧独立重算：回执里的限流条数须等于配额记录里的限流计数
        let mut throttle_receipts = 0u32;
        let mut i = 0usize;
        while i < c.receipts.len() {
            if c.receipts[i].is_throttled() {
                throttle_receipts += 1;
            }
            i += 1;
        }
        s.add(
            "B19-限流-限流丢弃必有回执且计数可对账",
            r.is_throttled()
                && line.contains("限流")
                // 回执文本必须能与其他三类区分
                && r.line() != Receipt::Accepted.line()
                && r.line() != Receipt::DegradedToHost.line()
                && r.line() != Receipt::Rejected(Reject::ParamCount).line()
                // 每次提交都留痕
                && receipts > 0
                // 独立对账：回执里的限流条数 == 配额记录里的限流计数
                && throttle_receipts == c.quota.throttled
                // 非限流类回执不带「限流」字样
                && !Receipt::Accepted.line().contains("限流"),
            "限流丢弃必留回执且回执文本可辨；回执中的限流条数与配额计数逐一对账",
        );
    }

    // --- 判据 8：拒绝回执必带原因（否则调用方无从纠正）----------------------
    {
        let mut c = DebugChannel::new();
        let bad = DebugMsg::new(MsgKind::Flush, alloc::vec![7]);
        let r = c.submit(&bad);
        let line = r.line();
        let reason = r.reason();
        // 三类拒绝的回执文本必须互不相同
        let l1 = Receipt::Rejected(Reject::ParamCount).line();
        let l2 = Receipt::Rejected(Reject::ParamValue).line();
        let l3 = Receipt::Rejected(Reject::ParamSemantics).line();
        s.add(
            "B19-回执-拒绝必带原因且非拒绝不带原因",
            r.is_rejected()
                && reason == Some(Reject::ParamSemantics)
                && line.contains("拒绝")
                && line.contains(Reject::ParamSemantics.zh())
                && c.reject_of(Reject::ParamSemantics) == 1
                // 非拒绝类回执的 reason 必须是 None（不能给占位原因）
                && Receipt::Accepted.reason().is_none()
                && Receipt::DegradedToHost.reason().is_none()
                && Receipt::Throttled(MsgKind::Flush).reason().is_none()
                && l1 != l2
                && l2 != l3
                && l1 != l3
                // 原因码互异
                && Reject::ParamCount.code() != Reject::ParamValue.code()
                && Reject::ParamValue.code() != Reject::ParamSemantics.code(),
            "拒绝回执带原因码与中文说明且三者互异；非拒绝类回执不携带任何原因",
        );
    }

    // --- 判据 9：通道不通则降级仅宿主侧并告知（不是丢弃）--------------------
    {
        let mut c = DebugChannel::new();
        c.set_state(ChannelState::Down);
        let log = DebugMsg::new(MsgKind::LogToggle, alloc::vec![1]);
        let r = c.submit(&log);
        // 前置：命令真的作用到宿主了（日志开关真的打开了）
        let host_applied = c.frame_log;
        let line = r.line();
        // 非法命令在不通状态下仍走拒绝（降级不等于放弃校验）
        let bad = DebugMsg::new(MsgKind::Flush, alloc::vec![9]);
        let r_bad = c.submit(&bad);
        s.add(
            "B19-降级-通道不通降级宿主侧并告知且不跳校验",
            r == Receipt::DegradedToHost
                // 回执告知已降级（否则用户以为命令生效到对端了）
                && line.contains("降级")
                && line.contains("宿主")
                // 命令仍作用于宿主（不是丢弃）
                && host_applied
                // 但对客不可达
                && !c.state.guest_reachable()
                // 三种状态的下标与标签互异
                && ChannelState::ALL.len() == STATE_COUNT
                && ChannelState::Up.ordinal() == 0
                && ChannelState::Down.ordinal() == 1
                && ChannelState::HostOnly.ordinal() == 2
                && ChannelState::Up.zh() != ChannelState::Down.zh()
                && ChannelState::Down.zh() != ChannelState::HostOnly.zh()
                // 非法命令在降级态仍被拒（校验不被跳过），
                // 且**没有被执行**——降级只改送到哪一侧，不改合不合法
                && r_bad.is_rejected()
                && r_bad.reason() == Some(Reject::ParamSemantics)
                // 非法命令没被记账（否则账本里混着「已处理的非法命令」）
                && c.handled_of(MsgKind::Flush) == 0
                && c.quota.accepted == 1,
            "通道不通时命令降级为仅宿主侧并回执告知、日志开关仍生效；降级态不跳过参数校验",
        );
    }

    // --- 判据 10：仅宿主侧态的语义（对客不可达，命令仍作用于宿主）-----------
    {
        let mut c = DebugChannel::new();
        c.set_state(ChannelState::HostOnly);
        let snap = DebugMsg::new(MsgKind::SnapshotExport, Vec::new());
        let r = c.submit(&snap);
        // 非法命令在仅宿主侧态仍被拒（校验未被跳过）
        let r_bad = c.submit(&DebugMsg::new(MsgKind::SnapshotExport, alloc::vec![1]));
        // 判据侧独立重算：仅宿主侧**走了半段通道**，故照占配额
        let want_used = ref_cost(MsgKind::SnapshotExport, 0);
        // 与完全不通态的**真实**区别：Down 压根没走通道故不占配额，
        // HostOnly 走了半段故要占。只说「有别」是空断言，得钉到数上。
        let mut down = DebugChannel::new();
        down.set_state(ChannelState::Down);
        down.submit(&snap);
        s.add(
            "B19-降级-仅宿主侧态回执告知且命令仍执行",
            r == Receipt::DegradedToHost
                && c.handled_of(MsgKind::SnapshotExport) == 1
                && !c.state.guest_reachable()
                && r.is_ok()
                && r_bad.is_rejected()
                // 半段通道 ⇒ 占配额（判据侧独立重算对齐）
                && c.quota.used_bytes == want_used
                && c.quota.accepted == 1
                // 完全不通 ⇒ 不占配额，但命令同样在宿主侧执行了
                && down.quota.used_bytes == 0
                && down.handled_of(MsgKind::SnapshotExport) == 1
                // 两者回执**相同**（都是降级告知），区别只在配额口径上——
                // 这正是「同回执不同语义」的要点，不能含糊过去
                && r == down.submit(&snap),
            "仅宿主侧态：对客不可达但命令仍作用于宿主；非法命令仍被拒；仅宿主侧占配额而完全不通不占（回执相同、配额口径不同）",
        );
    }

    // --- 判据 11：周期切换清配额但不清累计账本（两个口径必须分离）------------
    {
        let mut c = DebugChannel::new();
        let log = DebugMsg::new(MsgKind::LogToggle, alloc::vec![1]);
        c.submit(&log);
        c.submit(&log);
        let used_before = c.quota.used_bytes;
        let handled_before = c.handled_of(MsgKind::LogToggle);
        c.roll_period();
        // 配额清了（周期间账本）
        let quota_cleared = c.quota.used_bytes == 0
            && c.quota.throttled == 0
            && c.quota.safety_bypassed == 0
            && c.quota.accepted == 0;
        // 累计账本没清（全程账本）——若连它也清，「这个周期处理了多少」
        // 就永远查不出来
        let ledger_kept = c.handled_of(MsgKind::LogToggle) == handled_before
            && used_before > 0
            && handled_before == 2;
        s.add(
            "B19-周期-切换清配额但保留全程账本",
            quota_cleared
                && ledger_kept
                && c.receipts.len() == 2
                && c.quota.remaining() == QUOTA_PERIOD_BYTES,
            "周期切换清零配额四项；各类型累计计数与回执环形缓冲保留——周期间账本与全程账本分离",
        );
    }

    // --- 判据 12：环形缓冲有界且保留最近 RECEIPT_RING 条 ---------------------
    {
        let mut c = DebugChannel::new();
        let bad = DebugMsg::new(MsgKind::Flush, alloc::vec![1]);
        let mut i = 0usize;
        while i < RECEIPT_RING + 5 {
            c.submit(&bad);
            i += 1;
        }
        let bounded = c.receipts.len() == RECEIPT_RING;
        // 判据侧独立重算：提交总数由环深推导，不是随手写的字面量
        let total = (RECEIPT_RING + 5) as u32;
        s.add(
            "B19-回执-环形缓冲有界且保留最近条目",
            bounded
                // 累计拒绝计数不受环形丢出影响（丢的是历史，不是账）
                && c.reject_of(Reject::ParamSemantics) == total
                && c.quota.accepted == 0
                && c.quota.throttled == 0,
            "回执环形缓冲恒不超 RECEIPT_RING；超出后丢最旧，而累计拒绝计数不受环形丢出影响",
        );
    }

    // --- 判据 13：协议骨架可复用（F0239 沿用同抽象）--------------------------
    {
        let surface = protocol_surface();
        let mut names_dup = false;
        let mut i = 0usize;
        while i < surface.len() {
            let mut j = i + 1;
            while j < surface.len() {
                if surface[i].0 == surface[j].0 {
                    names_dup = true;
                }
                j += 1;
            }
            i += 1;
        }
        s.add(
            "B19-协议-骨架五族齐备可被下游沿用",
            surface.len() == 5
                && !names_dup
                // 线上编码集中在一处（F0239 换编码只改这一处）
                && MsgKind::LogToggle.wire() == 0x1001
                && MsgKind::Safety.wire() == 0x1F01
                // 解码入口是唯一入口（类型枚举与编码映射不是两套真相）
                && wire_kind(0x1002) == Some(MsgKind::Flush)
                // 五类都有中英标签（下游读屏直接复用）
                && MsgKind::ALL.iter().all(|k| !k.zh().is_empty() && !k.tag().is_empty()),
            "协议骨架含类型/消息/回执/配额/通道五族且互不重名；编码映射单点可换、下游读屏标签齐备",
        );
    }

    // --- 判据 14：读屏面板七行双语且不含用户内容 ---------------------------
    {
        let mut c = DebugChannel::new();
        // 造一个「敏感」参数值（显存地址量级），验证面板不含它
        let secret = (0x00AB_CDEFu32 & MAX_PARAM_VALUE) | 1; // 末位给 1 过布尔位校验
        let log = DebugMsg::new(MsgKind::LogToggle, alloc::vec![1]);
        c.submit(&log);
        let guest = DebugMsg::new(MsgKind::GuestEvent, alloc::vec![secret]);
        let r_guest = c.submit(&guest);
        let lines = c.a11y_lines();
        let joined = lines.join("|");
        // 独立重算：面板该报的两个聚合值
        let want_acc = c.quota.accepted;
        let want_rows = c.receipts.len();
        s.add(
            "B19-读屏-七行双语只报聚合计数不含用户内容",
            lines.len() == 7
                && lines.iter().all(|l| !l.is_empty())
                && lines.iter().any(|l| l.contains("channel state"))
                && lines.iter().any(|l| l.contains("throttled"))
                && lines.iter().any(|l| l.contains("safety bypassed"))
                && lines.iter().any(|l| l.contains("quota remaining"))
                // 聚合值必须准确（不是只查非空）
                && joined.contains(&format!("已接受 / accepted: {}", want_acc))
                && joined.contains(&format!("回执条数 / receipts: {}", want_rows))
                // 前置：这条客户事件**确实被处理了**（否则「面板无它」是空洞的）
                && r_guest == Receipt::Accepted
                && want_acc == 2
                // 参数值不出现在面板里
                && !joined.contains(&format!("{}", secret))
                // 也不出现参数字段名
                && !joined.contains("params")
                && !joined.contains("payload"),
            "面板七行中英双语、聚合值逐项准确；消息参数值（含敏感量级）不出现在面板中",
        );
    }

    // --- 判据 15：端到端——五类命令各走各路且计数彼此独立 -------------------
    {
        let mut c = DebugChannel::new();
        // 1) 正常接受
        let r_ok = c.submit(&DebugMsg::new(MsgKind::Flush, Vec::new()));
        // 2) 非法参数 → 拒绝
        let r_bad = c.submit(&DebugMsg::new(MsgKind::Flush, alloc::vec![1]));
        // 3) 日志开关真的改状态
        let r_log = c.submit(&DebugMsg::new(MsgKind::LogToggle, alloc::vec![1]));
        // 4) 客户事件回传
        let r_guest = c.submit(&DebugMsg::new(MsgKind::GuestEvent, alloc::vec![1]));
        // 5) 安全类
        let r_safe = c.submit(&DebugMsg::new(MsgKind::Safety, Vec::new()));
        // 判据侧独立重算：共提交 5 条，其中恰好 1 条被拒、4 条被接受
        let submitted = 5u32;
        let rejected = 1u32;
        s.add(
            "B19-端到端-五类命令各走各路计数独立",
            r_ok == Receipt::Accepted
                && r_bad.is_rejected()
                && r_log == Receipt::Accepted
                && r_guest == Receipt::Accepted
                && r_safe == Receipt::Accepted
                // 日志开关真的改了状态（否则这条命令等于空放）
                && c.frame_log
                // 各类型计数彼此独立：每一类恰好 1
                && c.handled_of(MsgKind::Flush) == 1
                && c.handled_of(MsgKind::LogToggle) == 1
                && c.handled_of(MsgKind::GuestEvent) == 1
                && c.handled_of(MsgKind::Safety) == 1
                // 未出现的一类计数为 0（不能有幽灵计数）
                && c.handled_of(MsgKind::SnapshotExport) == 0
                // 独立对账：接受 4 + 拒绝 1 == 提交 5
                && c.quota.accepted == submitted - rejected
                && c.reject_of(Reject::ParamSemantics) == rejected
                // 回执条数与提交数一致（每条提交必回一条）
                && c.receipts.len() == submitted as usize
                // 安全类单独计数
                && c.quota.safety_bypassed == 1,
            "接受/拒绝/日志开关/客户事件/安全类五路各归其位；接受数加拒绝数恰等于提交数；安全类单独计数",
        );
    }

    // --- 判据 16：两个上界常量**用字面量钉死**（不让判据跟着常量一起漂）---
    //
    // 变异 M06（`MAX_PARAMS` 8→16）与 M07（`MAX_PARAM_VALUE` 2^20→2^30）
    // 在判据 3/4 下**双双存活**，根因不是判据漏写，而是同源驱动恒真：
    // 那两条判据的语料全由常量表达式导出——`(0..(MAX_PARAMS + 1))`、
    // `MAX_PARAM_VALUE + 1`。常量一放宽，语料同步放宽，被测与判据
    // 一起挪到新边界上，结论恒为「越界被拒」⇒ 谓词改了等于没改。
    //
    // 一个从不被观察的阈值和一个错的阈值在行为上无法区分，所以本条
    // **绕开常量**、直接写死数值：9 个参数必拒、8 个必收；
    // 参数值 1048577 必拒、1048576 必收。两向都断，且用 `==` 不用 `>=`
    // （放宽常量时 1048576 仍会被判据抓到，不会被「至少」蒙过去）。
    {
        // 字面量 9/8：对应上界 8，**不引用 MAX_PARAMS**
        let nine: Vec<u32> = (0..9u32).collect();
        let eight: Vec<u32> = (0..8u32).collect();
        let m9 = DebugMsg::new(MsgKind::Flush, nine);
        let m8 = DebugMsg::new(MsgKind::Flush, eight);
        // 字面量 1048577/1048576：对应上界 1<<20，**不引用 MAX_PARAM_VALUE**
        // 用 GuestEvent（两参数、无布尔语义门）测值域，否则测到的是语义结果
        let v_over = DebugMsg::new(MsgKind::GuestEvent, alloc::vec![0u32, 1048577u32]);
        let v_at = DebugMsg::new(MsgKind::GuestEvent, alloc::vec![0u32, 1048576u32]);
        s.add(
            "B19-常量-两个上界以字面量钉死不随常量漂移",
            // 上界本身的值也要钉死：常量被改成 16 或 2^30 时本条立刻红，
            // 防止「放宽常量 + 判据跟着放宽」的同源漂移成为合法改动
            MAX_PARAMS == 8
                && MAX_PARAM_VALUE == 1048576
                // 9 个参数（越界一档）必拒，且原因是「个数」不是别的
                && m9.validate() == Err(Reject::ParamCount)
                // 恰好 8 个（在界上）必收——否则上界被写成 >= 就是误杀
                && m8.validate() == Ok(())
                // 值域两侧同理
                && v_over.validate() == Err(Reject::ParamValue)
                && v_at.validate() == Ok(()),
            "MAX_PARAMS==8 / MAX_PARAM_VALUE==1048576 被字面量钉死；9 参数与 1048577 必拒、8 参数与 1048576 必收",
        );
    }

    s
}
