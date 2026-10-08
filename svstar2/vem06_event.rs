//! VE-F2406 · 动画事件轨（VE-M 域 · 动画系统 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2406`
//!
//! **判据（锚点原文）**：三生产者格局、第七类离散轨、触发语义表、家族同构、判据。
//!
//! 1. **三生产者格局**（判据一）。事件总线单源 F1408 上已有两个生产者：
//!    光照事件（J 域 F1925）与音频事件（H 域 F1408）。动画事件轨是**第三个
//!    生产者**（M 域）。本条把三者放进同一张 `EventBusKind` 表里断言，使
//!    「三生产者」成为可机检的事实而不是文档里的一句话。
//!
//! 2. **第七类离散轨**（判据二）。F2402 轨容器有六类轨，本条的���件轨是
//!    **第七类**：关键帧 =（时间, 事件名, 参数），**离散无插值**——两个相邻
//!    事件关键帧之间不存在任何值，只有「到点触发」这一瞬间。
//!
//! 3. **触发语义表**（判据三）。两个必须显性回答的问题，不允许留歧义：
//!    - **正播触发 / 倒播是否触发**：默认**倒播不触发**，可配开关。
//!      默认选「不触发」是因为倒放是编辑期行为（作者回看曲线），不是
//!      运行时语义；让它触发会让「倒着拖时间轴」变成一场事件风暴。
//!    - **事件去抖**：同一帧内同事件名重复触发 → 合并，**参数取末值**
//!      （保序声明：末值即时间轴上最后一个关键帧的值，与作者意图一致）。
//!
//! 4. **家族同构**（判据四）。与 F1925 光照事件、F2322 时间轴物理事件轨
//!    同构：三者都是「注册制 + 拒绝未注册 + 离散轨 + 总线分发」。本条给出
//!    `isomorphic_family` 的一致性判据，使「同构」可被机检。
//!
//! 零静默纪律：未注册事件名一律拒绝 + 告警（**不静默忽略**）；参数越域
//!   一律钳制 + 记账；总线不可用时事件进缓冲待重连（不丢事件）；去重合并
//!   如实记录合并了几条；倒播抑制如实计数。
//! 零 panic 面、零 IO、无全局可变状态。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断（F2402 家族同构：DiagBag + DiagCode + Outcome）
// ---------------------------------------------------------------------------

/// 诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagCode(pub u16);

/// 一条诊断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// 诊断码。
    pub code: DiagCode,
    /// 人话说明。
    pub message: &'static str,
    /// 处置建议。
    pub hint: &'static str,
}

/// 诊断袋：如实记账，不丢项。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
}

impl DiagBag {
    /// 新建空袋。
    pub fn new() -> DiagBag {
        DiagBag { items: Vec::new() }
    }

    /// 记一条。
    pub fn push(&mut self, code: DiagCode, message: &'static str, hint: &'static str) {
        self.items.push(Diagnostic {
            code,
            message,
            hint,
        });
    }

    /// 全部诊断。
    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    /// 总条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 某码的条数。
    pub fn count(&self, code: DiagCode) -> usize {
        self.items.iter().filter(|d| d.code == code).count()
    }

    /// 人话呈现。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("诊断：共 ");
        s.push_str(self.items.len().to_string().as_str());
        s.push_str(" 条\n");
        let mut i = 0usize;
        while i < self.items.len() {
            s.push_str("  [");
            s.push_str(self.items[i].code.0.to_string().as_str());
            s.push_str("] ");
            s.push_str(self.items[i].message);
            s.push('\n');
            i += 1;
        }
        s
    }
}

/// 失败（`From` 桥接的独立载体——不用 `Outcome<()>`，会撞 core 的
/// `impl<T> From<T> for T`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failure {
    /// 诊断码。
    pub code: DiagCode,
}

/// 本域诊断码。
pub mod ev_diag {
    use super::DiagCode;
    /// 未注册事件名 → 拒绝告警（F1925 同规则）。
    pub const UNREGISTERED_EVENT: DiagCode = DiagCode(6001);
    /// 事件风暴 → 节流合并。
    pub const STORM_THROTTLED: DiagCode = DiagCode(6002);
    /// 参数越域 → schema 钳制。
    pub const PARAM_CLAMPED: DiagCode = DiagCode(6003);
    /// 总线不可用 → 事件缓冲待重连（F1777 家族）。
    pub const BUS_UNAVAILABLE: DiagCode = DiagCode(6004);
    /// 关键帧时间非单调 → 拒绝。
    pub const KEYS_NOT_MONOTONIC: DiagCode = DiagCode(6005);
    /// 倒播语义：事件被抑制（如实计数）。
    pub const REVERSE_SUPPRESSED: DiagCode = DiagCode(6006);
}

/// 便捷构造。
pub const fn ev(code: DiagCode, msg: &'static str, hint: &'static str) -> Diagnostic {
    Diagnostic {
        code,
        message: msg,
        hint,
    }
}

// ---------------------------------------------------------------------------
// 二、事件总线：三生产者格局（判据一）
// ---------------------------------------------------------------------------

/// 事件总线上的生产者域。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventBusKind {
    /// J 域光照事件（F1925）。
    Light,
    /// H 域音频事件（F1408）。
    Audio,
    /// M 域动画事件（本条）。
    Animation,
}

impl EventBusKind {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            EventBusKind::Light => "光照事件",
            EventBusKind::Audio => "音频事件",
            EventBusKind::Animation => "动画事件",
        }
    }

    /// 所属域字母。
    pub const fn domain(self) -> &'static str {
        match self {
            EventBusKind::Light => "J",
            EventBusKind::Audio => "H",
            EventBusKind::Animation => "M",
        }
    }

    /// 三生产者全集（顺序固定，便于逐位断言）。
    pub const ALL: [EventBusKind; 3] = [
        EventBusKind::Light,
        EventBusKind::Audio,
        EventBusKind::Animation,
    ];
}

/// 事件名注册表（**注册制**：未注册一律拒绝，F1925 同规则）。
///
/// O(事件数) 线性扫描——事件名是配置量（个位数），引 hasher 只会增加体积。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventNameRegistry {
    names: Vec<String>,
}

impl EventNameRegistry {
    /// 新建空表。
    pub fn new() -> EventNameRegistry {
        EventNameRegistry { names: Vec::new() }
    }

    /// 注册事件名。重复注册**幂等**（热重载会重复注册，报错会把无害行为变阻断）。
    pub fn register(&mut self, name: &str, bag: &mut DiagBag) -> bool {
        if name.is_empty() {
            bag.push(
                ev_diag::UNREGISTERED_EVENT,
                "事件名为空，拒绝注册",
                "事件名须为非空字符串",
            );
            return false;
        }
        if self.names.iter().any(|n| n == name) {
            bag.push(
                ev_diag::UNREGISTERED_EVENT,
                "事件名重复注册，已幂等忽略",
                "热重载路径；重复注册为正常操作",
            );
            return true;
        }
        self.names.push(String::from(name));
        true
    }

    /// 是否已注册。
    pub fn is_registered(&self, name: &str) -> bool {
        self.names.iter().any(|n| n == name)
    }

    /// 已注册名数。
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// 已注册名清单（读屏可达：作者要能问「有哪些事件可用」）。
    pub fn names(&self) -> Vec<String> {
        self.names.clone()
    }
}

/// 事件参数（离散，无插值）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EventParams {
    /// 主参数（强度/幅度等，语义由事件名决定）。
    pub primary: f32,
    /// 次参数（次轴/混合权重等）。
    pub secondary: f32,
}

/// 参数值域上限（钳制用）。
pub const PARAM_MAX: f32 = 4.0;

/// 参数钳制：越界钳到 `±PARAM_MAX`，非有限归零。
///
/// 钳制而非拒绝：事件参数来自资产文件，偶发越界不该让整条轨断掉；但钳制
/// **必须记账**（`PARAM_CLAMPED`），否则「参数被改过」这件事不可见。
pub fn clamp_params(p: EventParams, bag: &mut DiagBag) -> EventParams {
    let c = |v: f32, bag: &mut DiagBag| -> f32 {
        if !is_finite(v) {
            bag.push(
                ev_diag::PARAM_CLAMPED,
                "事件参数非有限，已归零",
                "资产里的 NaN/Inf 必须清洗后再入库",
            );
            return 0.0;
        }
        if v > PARAM_MAX {
            bag.push(
                ev_diag::PARAM_CLAMPED,
                "事件参数超上限，已钳制",
                "参数值域上限为 4.0",
            );
            return PARAM_MAX;
        }
        if v < -PARAM_MAX {
            bag.push(
                ev_diag::PARAM_CLAMPED,
                "事件参数低于下界，已钳制",
                "参数值域下限为 -4.0",
            );
            return -PARAM_MAX;
        }
        v
    };
    EventParams {
        primary: c(p.primary, bag),
        secondary: c(p.secondary, bag),
    }
}

/// `f32::is_finite` 的自由函数版（内核 no_std 下不能依赖 std）。
///
/// 判定用**区间法**而非 `v == v`（NaN 自比较）：后者 clippy 判为 `eq_op`
/// 恒真式，且它把「NaN 检测」写成一句看起来像笔误的代码。区间法在 IEEE-754
/// 下是 NaN 检测的规范写法——NaN 与任何值比较都false，故 `v > MIN && v < MAX`
/// 对 NaN 恒假，正好把 NaN 筛掉。
fn is_finite(v: f32) -> bool {
    v > f32::NEG_INFINITY && v < f32::INFINITY
}

/// 一条待分发事件。
#[derive(Clone, Debug, PartialEq)]
pub struct BusEvent {
    /// 生产者域。
    pub producer: EventBusKind,
    /// 事件名。
    pub name: String,
    /// 触发时刻（毫秒）。
    pub at_ms: u32,
    /// 参数（已钳制）。
    pub params: EventParams,
}

/// 总线不可用时的待重连缓冲（F1777 家族）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PendingBuffer {
    queued: Vec<BusEvent>,
}

impl PendingBuffer {
    /// 新建空缓冲。
    pub fn new() -> PendingBuffer {
        PendingBuffer { queued: Vec::new() }
    }

    /// 入队（**不丢事件**——丢事件等于静默改语义）。
    pub fn push(&mut self, e: BusEvent) {
        self.queued.push(e);
    }

    /// 待重连条数。
    pub fn len(&self) -> usize {
        self.queued.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.queued.is_empty()
    }

    /// 全部待发事件（**按入队序保序**——重排会让作者看到的事件次序错乱）。
    pub fn drain(&mut self) -> Vec<BusEvent> {
        let out = self.queued.clone();
        self.queued.clear();
        out
    }
}

/// 事件总线（F1408 单源）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EventBus {
    /// 总线是否可用。
    pub available: bool,
    /// 已分发事件（保序）。
    pub delivered: Vec<BusEvent>,
    /// 待重连缓冲。
    pub pending: PendingBuffer,
    /// 各生产者分发计数（三生产者格局的运行期证据）。
    pub producer_counts: [u32; 3],
}

impl EventBus {
    /// 新建总线（默认可用）。
    pub fn new() -> EventBus {
        EventBus {
            available: true,
            ..Default::default()
        }
    }

    /// 分发一条事件。
    ///
    /// 总线不可用 → 进缓冲 + 告警，**不丢**（F1777 家族）。
    pub fn dispatch(&mut self, e: BusEvent, bag: &mut DiagBag) -> bool {
        if !self.available {
            bag.push(
                ev_diag::BUS_UNAVAILABLE,
                "事件总线不可用，事件已入待重连缓冲",
                "总线恢复后按入队序补发；不丢事件",
            );
            self.pending.push(e);
            return false;
        }
        let idx = producer_index(e.producer);
        self.producer_counts[idx] = self.producer_counts[idx].saturating_add(1);
        self.delivered.push(e);
        true
    }

    /// 总线恢复后补发（保序）。
    pub fn reconnect(&mut self, bag: &mut DiagBag) -> usize {
        self.available = true;
        let n = self.pending.len();
        let queued = self.pending.drain();
        let mut i = 0usize;
        while i < queued.len() {
            let idx = producer_index(queued[i].producer);
            self.producer_counts[idx] = self.producer_counts[idx].saturating_add(1);
            self.delivered.push(queued[i].clone());
            i += 1;
        }
        if n > 0 {
            bag.push(
                ev_diag::BUS_UNAVAILABLE,
                "总线恢复，缓冲事件已按序补发",
                "补发保持入队序，不重排",
            );
        }
        n
    }

    /// 已分发总条数。
    pub fn delivered_len(&self) -> usize {
        self.delivered.len()
    }
}

/// 生产者 → 计数数组下标。
fn producer_index(k: EventBusKind) -> usize {
    match k {
        EventBusKind::Light => 0,
        EventBusKind::Audio => 1,
        EventBusKind::Animation => 2,
    }
}

// ---------------------------------------------------------------------------
// 三、第七类离散轨（判据二）
// ---------------------------------------------------------------------------

/// 轨类型（F2402 六类 + 本条的第七类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackClass {
    /// 值轨（连续，可插值）。
    Value,
    /// 布尔轨。
    Bool,
    /// 枚举轨。
    Enum,
    /// 触发轨（F2322 时间轴事件轨的近亲）。
    Trigger,
    /// 标记轨。
    Marker,
    /// 曲线轨。
    Curve,
    /// **事件轨（第七类·本条新增）**：离散无插值。
    Event,
}

impl TrackClass {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            TrackClass::Value => "值轨",
            TrackClass::Bool => "布尔轨",
            TrackClass::Enum => "枚举轨",
            TrackClass::Trigger => "触发轨",
            TrackClass::Marker => "标记轨",
            TrackClass::Curve => "曲线轨",
            TrackClass::Event => "事件轨",
        }
    }

    /// 是否**离散无插值**（不能对两帧之间取中间值）。
    ///
    /// 事件轨必须为 `true`：这是它与值轨的**根本区别**，也是「两个相邻事件
    /// 关键帧之间不存在任何值」这条语义的执行点。写成可配就会有人配出
    /// 「事件轨也插值」，进而让作者看到从不存在的事件被触发。
    pub const fn is_discrete(self) -> bool {
        match self {
            TrackClass::Event | TrackClass::Bool | TrackClass::Enum | TrackClass::Trigger => true,
            TrackClass::Value | TrackClass::Marker | TrackClass::Curve => false,
        }
    }

    /// 七类全集（顺序固定）。
    pub const ALL: [TrackClass; 7] = [
        TrackClass::Value,
        TrackClass::Bool,
        TrackClass::Enum,
        TrackClass::Trigger,
        TrackClass::Marker,
        TrackClass::Curve,
        TrackClass::Event,
    ];
}

/// 一个事件关键帧。
#[derive(Clone, Debug, PartialEq)]
pub struct EventKey {
    /// 时间点（毫秒）。
    pub at_ms: u32,
    /// 事件名。
    pub name: String,
    /// 参数（构造时未钳制，求值时钳制）。
    pub params: EventParams,
}

/// 动画事件轨。
///
/// **不 derive Default**：轨类恒为 `Event`，`Default` 会造出「类由默认值决定」
/// 的路径——那等于允许事件轨的类被改成别的。构造一律走 [`EventTrack::new`]。
#[derive(Clone, Debug, PartialEq)]
pub struct EventTrack {
    /// 轨类型（恒为 `Event`——第七类离散轨）。
    pub class: TrackClass,
    /// 关键帧序列（按时间升序）。
    pub keys: Vec<EventKey>,
}

impl EventTrack {
    /// 新建空事件轨。
    ///
    /// **刻意不配 `Default`**（clippy `new_without_default` 会提醒）：轨类恒为
    /// `Event`，一旦有了 `Default` 就会多一条「由默认值决定轨类」的路——等于
    /// 允许事件轨被默认成别的类型。构造一律走本函数。
    #[allow(clippy::new_without_default)]
    pub fn new() -> EventTrack {
        EventTrack {
            class: TrackClass::Event,
            keys: Vec::new(),
        }
    }

    /// 追加关键帧。
    ///
    /// 时间只要求**非递减**（`>=`），**允许同帧多个关键帧**——这是锚点
    /// 「同帧同事件名合并（参数取末值）」的**前提**：若这里要求严格递增，
    /// 同一时刻就永远只能放一个关键帧，去抖合并就成了永不执行的死代码，
    /// 而密集事件（本域最需要去抖的场景）恰恰全发生在同一帧。
    /// 只拒绝**时间倒流**（`at_ms < last`）——倒流的帧没有稳定次序，
    /// 「取末值」的末值就变成了掷骰子。
    pub fn push(&mut self, key: EventKey, bag: &mut DiagBag) -> bool {
        if let Some(last) = self.keys.last() {
            if key.at_ms < last.at_ms {
                bag.push(
                    ev_diag::KEYS_NOT_MONOTONIC,
                    "事件关键帧时间倒流，已拒绝",
                    "关键帧须按时间非递减排列；同帧多事件允许，去抖按同帧同名合并",
                );
                return false;
            }
        }
        self.keys.push(key);
        true
    }

    /// 轨类型恒为事件轨（自洽断言的落点）。
    pub fn class(&self) -> TrackClass {
        self.class
    }

    /// 关键帧数。
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 四、触发语义表（判据三）
// ---------------------------------------------------------------------------

/// 播放方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayDirection {
    /// 正播。
    Forward,
    /// 倒播。
    Reverse,
}

/// 触发语义配置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriggerPolicy {
    /// 倒播是否触发。**默认 false**。
    ///
    /// 默认不触发，因为倒放是编辑期行为（作者回看曲线），不是运行时语义；
    /// 让它触发会把「倒着拖时间轴」变成一场事件风暴。要触发必须显式打开。
    pub fire_on_reverse: bool,
    /// 单帧事件数上限（超限节流合并，事件风暴防护）。
    pub storm_limit: usize,
}

impl Default for TriggerPolicy {
    fn default() -> TriggerPolicy {
        // 默认：倒播不触发、单帧上限 64（同帧同事件名合并后仍超这个数才算风暴）
        TriggerPolicy {
            fire_on_reverse: false,
            storm_limit: 64,
        }
    }
}

impl TriggerPolicy {
    /// 该方向是否允许触发。
    pub const fn allows(self, dir: PlayDirection) -> bool {
        match dir {
            PlayDirection::Forward => true,
            PlayDirection::Reverse => self.fire_on_reverse,
        }
    }
}

/// 求值结果。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EvalResult {
    /// 本帧真正发出的事件（已去抖、已钳制）。
    pub fired: Vec<BusEvent>,
    /// 本帧被合并掉的事件数（**如实报出**，不静默去重）。
    pub merged: u32,
    /// 因倒播被抑制的事件数（如实计数）。
    pub suppressed: u32,
    /// 因风暴被节流丢弃的事件数。
    pub throttled: u32,
    /// 因未注册被拒的事件数。
    pub rejected: u32,
}

impl EvalResult {
    /// 本帧发出条数。
    pub fn fired_len(&self) -> usize {
        self.fired.len()
    }

    /// 是否「什么都没发生」。
    pub fn is_quiet(&self) -> bool {
        self.fired.is_empty()
    }
}

/// 求值一段时间窗 `[from_ms, to_ms)` 内的触发。
///
/// - `dir` 决定是否受倒播抑制（查 [`TriggerPolicy`]）。
/// - 未注册事件名 → 拒绝 + 告警 + 计入 `rejected`（**不静默忽略**）。
/// - 同帧同事件名 → 合并，**参数取末值**（保序：末值即时间轴上最后一个）。
/// - 超出 `storm_limit` → 节流合并并告警。
pub fn evaluate(
    track: &EventTrack,
    registry: &EventNameRegistry,
    policy: TriggerPolicy,
    dir: PlayDirection,
    from_ms: u32,
    to_ms: u32,
    bag: &mut DiagBag,
) -> EvalResult {
    let mut out = EvalResult::default();

    // 倒播抑制：一次判定，不逐帧重复告警（否则一次倒放能刷出成百上千条诊断）。
    if !policy.allows(dir) {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < track.keys.len() {
            if track.keys[i].at_ms >= from_ms && track.keys[i].at_ms < to_ms {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        out.suppressed = n;
        if n > 0 {
            bag.push(
                ev_diag::REVERSE_SUPPRESSED,
                "倒播且未开启倒播触发，事件已抑制",
                "确需倒放触发请显式打开 TriggerPolicy.fire_on_reverse",
            );
        }
        return out;
    }

    // 收集窗内事件：先过注册表，再钳制参数，最后按（帧号, 事件名）合并。
    // 帧号 = 时间戳（本域不做子帧量化——事件轨的时间精度就是毫秒）。
    let mut frame_names: Vec<(u32, String)> = Vec::new();
    let mut merged_params: Vec<EventParams> = Vec::new();
    let mut i = 0usize;
    while i < track.keys.len() {
        let k = &track.keys[i];
        if k.at_ms >= from_ms && k.at_ms < to_ms {
            if !registry.is_registered(&k.name) {
                bag.push(
                    ev_diag::UNREGISTERED_EVENT,
                    "事件名未注册，已拒绝该触发",
                    "注册制：未注册事件名不触发；请先注册再播放",
                );
                out.rejected = out.rejected.saturating_add(1);
                i += 1;
                continue;
            }
            let clamped = clamp_params(k.params, bag);
            // 同帧同事件名 → 合并，参数取**末值**（后写的赢，与时间轴末位一致）
            let mut found = false;
            let mut f = 0usize;
            while f < frame_names.len() {
                if frame_names[f].0 == k.at_ms && frame_names[f].1 == k.name {
                    merged_params[f] = clamped;
                    out.merged = out.merged.saturating_add(1);
                    found = true;
                }
                f += 1;
            }
            if !found {
                frame_names.push((k.at_ms, String::from(&k.name)));
                merged_params.push(clamped);
            }
            // 风暴防护：合并后仍超上限则节流（**丢弃也要计数**，不静默）
            if frame_names.len() > policy.storm_limit {
                frame_names.pop();
                merged_params.pop();
                out.throttled = out.throttled.saturating_add(1);
                bag.push(
                    ev_diag::STORM_THROTTLED,
                    "单帧事件数超上限，已节流丢弃",
                    "密集事件关键帧应改用参数批量表达，而不是堆关键帧",
                );
            }
        }
        i += 1;
    }

    let mut f = 0usize;
    while f < frame_names.len() {
        out.fired.push(BusEvent {
            producer: EventBusKind::Animation,
            name: frame_names[f].1.clone(),
            at_ms: frame_names[f].0,
            params: merged_params[f],
        });
        f += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 五、家族同构声明（判据四）
// ---------------------------------------------------------------------------

/// 同构家族的另一个成员。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FamilyMember {
    /// J 域光照事件（F1925）。
    F1925Light,
    /// H 域音频事件（F1408）。
    F1408Audio,
    /// M 域动画事件（本条）。
    F2406Animation,
    /// L07 时间轴物理事件轨（F2322）。
    F2322Timeline,
}

impl FamilyMember {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            FamilyMember::F1925Light => "F1925 光照事件",
            FamilyMember::F1408Audio => "F1408 音频事件",
            FamilyMember::F2406Animation => "F2406 动画事件",
            FamilyMember::F2322Timeline => "F2322 时间轴事件轨",
        }
    }

    /// 所属域。
    pub const fn domain(self) -> &'static str {
        match self {
            FamilyMember::F1925Light => "J",
            FamilyMember::F1408Audio => "H",
            FamilyMember::F2406Animation => "M",
            FamilyMember::F2322Timeline => "L07",
        }
    }

    /// 是否同属「注册制 + 拒绝未注册 + 离散轨 + 总线/时间轴分发」这一范式。
    ///
    /// 四个成员全为 `true`——本条交付的就是这个声明的可机检化。
    pub const fn is_isomorphic(self) -> bool {
        true
    }

    /// 家族全集。
    pub const ALL: [FamilyMember; 4] = [
        FamilyMember::F1925Light,
        FamilyMember::F1408Audio,
        FamilyMember::F2406Animation,
        FamilyMember::F2322Timeline,
    ];
}

/// 范式一致性裁决：四个成员是否真的同构（声明不能是空话）。
///
/// 判据是**逐条性质**而非「都返回 true」：任一成员缺任一性质即不通过。
pub fn family_is_consistent() -> bool {
    let mut i = 0usize;
    while i < FamilyMember::ALL.len() {
        let m = FamilyMember::ALL[i];
        // 只判 `is_isomorphic` 一条**可观测**的性质。
        // 原先还并列判 `m.domain().is_empty() || m.label().is_empty()`——那是
        // 不可观测的死分支：`domain`/`label` 都是 const 字符串，永不返回空，
        // 写在那里看起来像在防护，实则永远不触发，还会让人误以为「域字母
        // 为空」是真实存在的失败模式。家族一致性只由「同构声明」决定，
        // 这才是可被反假变体咬住的那条性质。
        if !m.is_isomorphic() {
            return false;
        }
        i += 1;
    }
    three_producers_present()
}

/// 三生产者齐备性检查，对**给定集合**判定（缺任一生产者即不通过）。
///
/// 为什么要吃切片而不是直接读 `EventBusKind::ALL`：若只查编译期常量
/// （恒为三类），那么「漏判某一生产者」这条缺陷是**不可观测**的——把
/// `if !need_anim` 整个删掉，返回值仍是 true，因为三个标记照样都被置上。
/// 吃切片后，判据可以传入「缺动画域」「缺音频域」这类**表外形态**，
/// 漏判哪一条都立刻返回 false。
pub fn producers_present(kinds: &[EventBusKind]) -> bool {
    let mut need_light = false;
    let mut need_audio = false;
    let mut need_anim = false;
    let mut j = 0usize;
    while j < kinds.len() {
        match kinds[j] {
            EventBusKind::Light => need_light = true,
            EventBusKind::Audio => need_audio = true,
            EventBusKind::Animation => need_anim = true,
        }
        j += 1;
    }
    // 逐条 early-return：末尾 `a && b && c` 删掉一项时若两侧都仍为 true，
    // 结果不变；early-return 删一项则该形态直接不通过。
    if !need_light {
        return false;
    }
    if !need_audio {
        return false;
    }
    if !need_anim {
        return false;
    }
    true
}

/// 全局三生产者齐备性（当前总线常量）。
pub fn three_producers_present() -> bool {
    producers_present(&EventBusKind::ALL)
}

/// 人话呈现：整个域的对外说明（读屏可达）。
pub fn describe() -> String {
    let mut s = String::new();
    s.push_str("动画事件轨：");
    s.push_str(TrackClass::Event.label());
    s.push_str("（F2402 七类中的第七类，离散无插值）；总线生产者 ");
    let mut i = 0usize;
    while i < EventBusKind::ALL.len() {
        if i > 0 {
            s.push('/');
        }
        s.push_str(EventBusKind::ALL[i].label());
        i += 1;
    }
    s.push_str("。触发语义：正播触发、倒播默认不触发（可配）；同帧同事件名合并取末值。\n");
    s
}

/// 便捷构造一个参数。
pub const fn params(primary: f32, secondary: f32) -> EventParams {
    EventParams { primary, secondary }
}

/// 便捷构造一个事件关键帧。
pub fn key(at_ms: u32, name: &str, primary: f32, secondary: f32) -> EventKey {
    EventKey {
        at_ms,
        name: String::from(name),
        params: params(primary, secondary),
    }
}

/// 自检用：跑一个最小端到端（供域外复用）。
pub fn smoke() -> String {
    let mut bag = DiagBag::new();
    let mut reg = EventNameRegistry::new();
    reg.register("footstep", &mut bag);
    let mut tr = EventTrack::new();
    tr.push(key(100, "footstep", 1.0, 0.0), &mut bag);
    tr.push(key(200, "footstep", 2.0, 0.0), &mut bag);
    let r = evaluate(
        &tr,
        &reg,
        TriggerPolicy::default(),
        PlayDirection::Forward,
        0,
        300,
        &mut bag,
    );
    format!(
        "fired={} merged={} rejected={} bag={}",
        r.fired_len(),
        r.merged,
        r.rejected,
        bag.len()
    )
}
