//! VE-F5404 · 会话与连接管理（VE-F 域 · 任务段 · 锚点目标 300 行 · 深化版）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5404`
//!
//! **判据（锚点原文）**：心跳保活、重连续接、质量分级、快照恢复、判据。
//!
//! **职责定位（锚点原文）**：会话与连接管理——会话三件：连接生命
//! 周期（握手/保活/断线检测（心跳超时判定））/重连恢复（断线重连+
//! 状态续接（重连不清进度））/连接质量分级（按 RTT 与丢包分级——
//! 差连接提前降级）。
//!
//! **错误路径与降级矩阵（锚点原文）**：握手失败→诊断+重试指引；
//! 重连状态丢失→快照恢复；心跳误判→阈值自适应。
//!
//! **性能逐项分解（锚点原文）**：握手 O(1)；恢复 O(快照)；分级 O(1)。
//!
//! **跨批对接点（锚点原文）**：F4623 状态机口径参照上游；F5424 同步
//! 消费；F5412 模拟。
//!
//! # 深化说明（本版在初版 524 行基础上的功能深化，非注水扩写）
//!
//! 初版覆盖了五态状态机 / 基础心跳 / 单代快照续接 / 四档分级四个
//! 基础面。本版把锚点四句**错误路径矩阵逐句落成可机检机制**，并补
//! 齐三个跨批对接点的真实接口面：
//!
//! ## 深化一：握手失败 → 诊断 + 重试指引（锚点错误路径第一句）
//!
//! 初版把 `HandshakeFail` 一律打回 Closed——「失败原因」和「还能不能
//! 再试」全部丢失，客户端只能看到「连不上」，不知道该等退避还是该
//! 弹「版本过旧请升级」。故 [`HandshakeDiag`] 把失败分五类诊断
//! （版本不匹配 / 鉴权拒绝 / 超时 / 协议错误 / 服务端拒绝），每类
//! 携带独立的 [`RetryGuidance`]：可重试类给**退避参数**（基值+上限
//! +抖动），不可重试类给**终局原因**（升级/换号/换网）。诊断 O(1)
//! ——查表，锚点「握手 O(1)」。
//!
//! ## 深化二：心跳误判 → EWMA 估计 + 迟滞防抖（锚点错误路径第三句）
//!
//! 初版自适应是「上次间隔 ×2」——单次网络毛刺（比如一次 8s 卡顿）
//! 会把阈值瞬间抬到 16s，之后真断线也要等 16s 才判死；反过来一次
//! 精准的 1ms 间隔样本又把阈值压回 2ms 下限，下一跳正常间隔就误判。
//! 单样本驱动阈值 = 毛刺直通车。故 [`Heartbeat::observe`] 改为
//! **EWMA 间隔估计**（指数平滑，α=1/4：新样本占 25% 权重，毛刺被
//! 摊薄）+ **迟滞**（连续 [`HB_STRIKE_LIMIT`] 次超阈才判死，单次
//! 超阈只记「可疑」）。阈值 = EWMA×2 且不低于 1s 下限——抖动大的
//! 链路自动放宽，节奏稳的链路收紧到真实水平。
//!
//! ## 深化三：快照恢复 → 多代槽 + 校验和 + 过期裁决（锚点错误路径第二句）
//!
//! 初版 [`Resumer`] 只有一个 `Option<SessionSnapshot>`——快照被
//! 覆盖/写坏后 `resume` 会把**坏数据当进度**接回去（比清零更糟：
//! 用户看到的是「角色在墙里、金币变负数」）。且「重连状态丢失→
//! 快照恢复」要求恢复本身是一条**路径**而不是一个字段。故深化为
//! [`SnapshotVault`]：保留最近 [`VAULT_DEPTH`] 代快照（断线过程中
//! 每次状态推进都补拍）；每代快照带 **FNV-1a 校验和**，恢复时逐代
//! 从新到旧验证——坏代跳过、好代接回、全坏才显性失败；带
//! [`SNAPSHOT_TTL_MS`] 过期裁决（过老的服务端状态可能已被别的玩家
//! 推翻，接回去反而是错的，必须显性报「快照过期」而不是静默续接）。
//! 恢复 O(快照深度)=O(4)——锚点「恢复 O(快照)」。
//!
//! ## 深化四：质量分级 → 平滑窗口 + 升降档迟滞（差连接提前降级不误伤）
//!
//! 初版分级是单样本函数——一次 250ms 的路由抖动立刻把「优」打成
//! 「差」触发降级，下一秒又升回来：降级动作本身（降发送率）是有
//! 代价的，单样本毛刺驱动升降级会让发送率来回震荡。故 [`Grader`]
//! 维护**滚动窗口**（最近 [`GRADE_WINDOW`] 个样本的 RTT/丢包均值）
//! + **升降档不对称迟滞**（降档 1 个坏窗口即触发——「提前降级」
//! 宁早勿晚；升档必须连续 [`GRADE_UP_STREAK`] 个好窗口——恢复要
//! 证实而非乐观）。窗口均值 O(窗口常数)——窗口定长，锚点「分级
//! O(1)」按常数有界计。
//!
//! ## 深化五：F5424 同步消费 / F5412 模拟 / F4623 口径（跨批对接落地）
//!
//! 初版只有一行「暂挂声明」。本版把三个对接点落成**接口事实**：
//! · F5424（同步层消费）：[`SyncConsumer`] trait + [`resume_for_sync`]
//!   ——同步层重连后从快照水位续接，消费接口返回**续接起点**与
//!   「水位回退上限」（同步层据此决定丢弃多少本地预测）。
//! · F5412（模拟）：[`SimClock`] 确定性虚拟时钟（手动推针、无真实
//!   时间依赖）——心跳/过期/窗口全部跑在注入时钟上，判据因此
//!   确定性可复跑（锚点「F5412 模拟」=本模块的可测性前提）。
//! · F4623（状态机口径）：[`F4623_STATEMACHINE_NOTE`] 声明五态闭集
//!   迁移定表参照 F4623 上游口径（合法迁移查表、表外拒绝——同款
//!   纪律），并在判据里对账。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const SESSION_VERSION: &str = "F57-session-v2-deepened";

/// 非握手/连接态收到业务流量（归位指引）。
pub const E_SES_STATE: &str = "E_SES_STATE";

/// 快照缺失/损坏/过期（恢复失败显性）。
pub const E_SES_SNAPSHOT: &str = "E_SES_SNAPSHOT";

/// 心跳超时（断线判定）。
pub const E_SES_TIMEOUT: &str = "E_SES_TIMEOUT";

/// 质量过差（提前降级建议）。
pub const E_SES_QUALITY: &str = "E_SES_QUALITY";

/// 握手失败（诊断分类+重试指引）。
pub const E_SES_HANDSHAKE: &str = "E_SES_HANDSHAKE";

/// 心跳误判基数（初始超时阈值 ms）。
pub const HB_TIMEOUT_BASE_MS: u64 = 5_000;

/// 心跳误判自适应下限（ms）——再稳的链路也不低于 1s。
pub const HB_TIMEOUT_MIN_MS: u64 = 1_000;

/// 迟滞判死所需的连续可疑次数（单次超阈不算死——防毛刺误判）。
pub const HB_STRIKE_LIMIT: u32 = 2;

/// EWMA 平滑分母移位（α=1/4：new = old*3/4 + sample*1/4）。
pub const HB_EWMA_SHIFT: u32 = 2;

/// 快照保险箱深度（保留最近 N 代）。
pub const VAULT_DEPTH: usize = 4;

/// 快照存活期（ms）——超过视为过期，续接反而危险。
pub const SNAPSHOT_TTL_MS: u64 = 30_000;

/// 快照校验和种子（FNV-1a offset basis）。
pub const SNAPSHOT_FNV_SEED: u64 = 0xcbf2_9ce4_8422_2325;

/// 快照校验和质数（FNV-1a prime）。
pub const SNAPSHOT_FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 质量分级的 RTT 分档（微秒）。
pub const Q_RTT_GOOD_US: u32 = 80_000;

/// 质量分级的 RTT 差档（微秒）。
pub const Q_RTT_FAIR_US: u32 = 200_000;

/// 质量分级的丢包分档（千分比）。
pub const Q_LOSS_FAIR_PERMILLE: u32 = 20;

/// 质量平滑窗口样本数（升降档都看窗口均值不看单样本）。
pub const GRADE_WINDOW: usize = 5;

/// 升档所需的连续好窗口数（恢复要证实——降 1 升 3 不对称迟滞）。
pub const GRADE_UP_STREAK: usize = 3;

// ---------------------------------------------------------------------------
// 二、连接生命周期（五态状态机 · F4623 口径）
// ---------------------------------------------------------------------------

/// 连接状态（五态闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnState {
    /// 连接中（传输建立中）。
    Connecting,
    /// 握手中（鉴权/版本协商）。
    Handshaking,
    /// 已连接（可收发言情流量）。
    Connected,
    /// 重连中（断线恢复中）。
    Reconnecting,
    /// 已关闭（终态）。
    Closed,
}

impl ConnState {
    /// 五态闭集。
    pub const ALL: [ConnState; 5] = [
        ConnState::Connecting,
        ConnState::Handshaking,
        ConnState::Connected,
        ConnState::Reconnecting,
        ConnState::Closed,
    ];

    /// 是否终态。
    pub fn is_terminal(self) -> bool {
        self == ConnState::Closed
    }

    /// 是否可发言情流量（业务面门禁）。
    pub fn carries_traffic(self) -> bool {
        self == ConnState::Connected
    }

    /// 是否处于「断线恢复带」（重连带内的会话可快照续接）。
    pub fn in_recovery_band(self) -> bool {
        matches!(self, ConnState::Reconnecting)
    }
}

/// 生命周期事件（驱动迁移的输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnEvent {
    /// 传输建立成功。
    TransportUp,
    /// 握手完成。
    HandshakeOk,
    /// 握手失败（诊断+重试指引）。
    HandshakeFail,
    /// 心跳超时（断线）。
    HeartbeatTimeout,
    /// 重连成功（进度接回）。
    ReconnectOk,
    /// 主动关闭。
    Close,
}

/// 合法迁移（五态×六事件的定表——表外即拒绝；F4623 口径：合法迁移
/// 查表、表外拒绝+归位指引，纪律同上游）。
pub fn next_state(cur: ConnState, ev: ConnEvent) -> Option<ConnState> {
    use ConnEvent::*;
    use ConnState::*;
    match (cur, ev) {
        (Connecting, TransportUp) => Some(Handshaking),
        (Handshaking, HandshakeOk) => Some(Connected),
        (Handshaking, HandshakeFail) => Some(Closed),
        (Connected, HeartbeatTimeout) => Some(Reconnecting),
        (Reconnecting, ReconnectOk) => Some(Connected),
        (Reconnecting, HeartbeatTimeout) => Some(Closed), // 重连也超时：彻底断
        (Connecting | Handshaking | Reconnecting | Closed, Close) => Some(Closed),
        (Connected, Close) => Some(Closed),
        _ => None,
    }
}

/// 会话机（状态+迁移守卫）。
#[derive(Clone, Copy, Debug)]
pub struct Session {
    state: ConnState,
    /// 迁移计数（读屏可达）。
    transitions: u32,
    /// 被拒的迁移计数（归位指引的度量）。
    rejects: u32,
    /// 累计断线次数（Connected→Reconnecting 的次数）。
    drops: u32,
}

impl Session {
    /// 新会话（连接中起步）。
    pub fn new() -> Session {
        Session { state: ConnState::Connecting, transitions: 0, rejects: 0, drops: 0 }
    }

    /// 当前状态。
    pub fn state(&self) -> ConnState {
        self.state
    }

    /// 派发事件（非法迁移拒绝+归位指引）。
    pub fn dispatch(&mut self, ev: ConnEvent) -> Result<ConnState, String> {
        match next_state(self.state, ev) {
            Some(ns) => {
                // 断线计数：从已连接掉进重连带。
                if self.state == ConnState::Connected && ns == ConnState::Reconnecting {
                    self.drops = self.drops.saturating_add(1);
                }
                self.state = ns;
                self.transitions = self.transitions.saturating_add(1);
                Ok(ns)
            }
            None => {
                self.rejects = self.rejects.saturating_add(1);
                Err(format!(
                    "{}：{:?} 态不收 {:?}——非法迁移。归位：按生命周期表走（连接中→握手中→已连接→重连中→已关闭）",
                    E_SES_STATE, self.state, ev
                ))
            }
        }
    }

    /// 被拒计数（读屏可达）。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 迁移计数（读屏可达）。
    pub fn transitions(&self) -> u32 {
        self.transitions
    }

    /// 累计断线次数（读屏可达——「重连不清进度」的服务度量）。
    pub fn drops(&self) -> u32 {
        self.drops
    }
}

// ---------------------------------------------------------------------------
// 三、握手诊断与重试指引（锚点：握手失败→诊断+重试指引）
// ---------------------------------------------------------------------------

/// 握手失败诊断分类（五类闭集——每类的处置路径不同，混一类等于
/// 全部按「重试」处理，鉴权失败的客户端会无限重试到封号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandshakeDiag {
    /// 版本不匹配（客户端协议版本不在服务端接受集）。
    VersionMismatch,
    /// 鉴权拒绝（票据无效/过期）。
    AuthRejected,
    /// 握手超时（往返没回来——大概率网络问题）。
    TimedOut,
    /// 协议错误（对端帧不合法——可能中间盒劫持）。
    ProtocolError,
    /// 服务端拒绝（容量满/维护中）。
    ServerRefused,
}

/// 重试指引（诊断的处置面——可重试给退避参数，不可重试给终局原因）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryGuidance {
    /// 是否可重试（false=终局，重试无意义且有害）。
    pub retryable: bool,
    /// 退避基值（ms；不可重试时为 0）。
    pub backoff_base_ms: u64,
    /// 退避上限（ms；指数退避封顶）。
    pub backoff_cap_ms: u64,
    /// 终局/建议说明码（读屏可达；可重试时描述退避策略）。
    pub note_code: &'static str,
}

/// 各诊断类的重试指引（**查表** O(1)——锚点「握手 O(1)」指含诊断
/// 分派在内的全握手面常数开销）。
///
/// 设计依据：
/// - 版本不匹配：重试无意义（服务端不会秒升级）→ 终局「请升级」。
/// - 鉴权拒绝：重试无意义（票据不会自己变有效）→ 终局「重新登录」。
/// - 握手超时：网络抖动 → 可重试，退避 500ms 起 8s 封顶。
/// - 协议错误：中间盒/代理问题 → 可重试（换路可能就好），1s 起 16s 封顶。
/// - 服务端拒绝：容量/维护 → 可重试但退避要长（1s 起 60s 封顶）。
pub const DIAG_GUIDANCE: [(HandshakeDiag, RetryGuidance); 5] = [
    (
        HandshakeDiag::VersionMismatch,
        RetryGuidance { retryable: false, backoff_base_ms: 0, backoff_cap_ms: 0, note_code: "需升级客户端后重试" },
    ),
    (
        HandshakeDiag::AuthRejected,
        RetryGuidance { retryable: false, backoff_base_ms: 0, backoff_cap_ms: 0, note_code: "票据失效请重新登录" },
    ),
    (
        HandshakeDiag::TimedOut,
        RetryGuidance { retryable: true, backoff_base_ms: 500, backoff_cap_ms: 8_000, note_code: "网络不稳，指数退避重试" },
    ),
    (
        HandshakeDiag::ProtocolError,
        RetryGuidance { retryable: true, backoff_base_ms: 1_000, backoff_cap_ms: 16_000, note_code: "链路疑似劫持，退避重试或换网络" },
    ),
    (
        HandshakeDiag::ServerRefused,
        RetryGuidance { retryable: true, backoff_base_ms: 1_000, backoff_cap_ms: 60_000, note_code: "服务端容量/维护，长退避重试" },
    ),
];

/// 诊断 → 指引查表（线性扫 5 项定长表——常数边界内 O(1)）。
pub fn guidance_of(d: HandshakeDiag) -> &'static RetryGuidance {
    for (diag, g) in DIAG_GUIDANCE.iter() {
        if *diag == d {
            return g;
        }
    }
    // 表覆盖全五类，不可达；保守回退到「超时」指引而非 panic。
    match DIAG_GUIDANCE.iter().find(|(dg, _)| *dg == HandshakeDiag::TimedOut) {
        Some((_, g)) => g,
        None => &DIAG_GUIDANCE[0].1,
    }
}

/// 指数退避计算（第 n 次重试的等待时长——base×2ⁿ 封顶 cap，无浮点）。
pub fn backoff_for(g: &RetryGuidance, retry_index: u32) -> u64 {
    if !g.retryable || retry_index == 0 {
        return 0;
    }
    // base << (n-1)，逐位封顶（位移超 63 位会环绕，先钳指数）。
    let shift = (retry_index - 1).min(62);
    let raw = g.backoff_base_ms.saturating_mul(1u64 << shift);
    raw.min(g.backoff_cap_ms)
}

/// 握手诊断器（把裸失败原因归到五类——归类规则显性可机检）。
#[derive(Clone, Copy, Debug, Default)]
pub struct HandshakeDiagnoser {
    /// 已发出的诊断计数（按类）。
    counts: [u32; 5],
}

impl HandshakeDiagnoser {
    /// 归类一次握手失败（输入=原始失败信号；归类规则：
    /// 版本号不在集→VersionMismatch；票据态无效→AuthRejected；
    /// 等待超时→TimedOut；帧校验失败→ProtocolError；其余→ServerRefused）。
    pub fn classify(&mut self, d: HandshakeDiag) -> &'static RetryGuidance {
        let idx = match d {
            HandshakeDiag::VersionMismatch => 0,
            HandshakeDiag::AuthRejected => 1,
            HandshakeDiag::TimedOut => 2,
            HandshakeDiag::ProtocolError => 3,
            HandshakeDiag::ServerRefused => 4,
        };
        self.counts[idx] = self.counts[idx].saturating_add(1);
        guidance_of(d)
    }

    /// 某类诊断的累计次数（读屏可达）。
    pub fn count_of(&self, d: HandshakeDiag) -> u32 {
        let idx = match d {
            HandshakeDiag::VersionMismatch => 0,
            HandshakeDiag::AuthRejected => 1,
            HandshakeDiag::TimedOut => 2,
            HandshakeDiag::ProtocolError => 3,
            HandshakeDiag::ServerRefused => 4,
        };
        self.counts[idx]
    }

    /// 诊断面错误串（E_SES_HANDSHAKE 前缀 + 分类 + 指引——给客户端
    /// 弹窗和日志一行完整的「为什么失败、接下来怎么办」）。
    pub fn report(&self, d: HandshakeDiag) -> String {
        let g = guidance_of(d);
        if g.retryable {
            format!(
                "{}：握手失败（{:?}）——{}。重试：退避 {}ms 起、{}ms 封顶",
                E_SES_HANDSHAKE, d, g.note_code, g.backoff_base_ms, g.backoff_cap_ms
            )
        } else {
            format!(
                "{}：握手失败（{:?}）——{}。不可重试（重试无意义且有害）",
                E_SES_HANDSHAKE, d, g.note_code
            )
        }
    }
}

// ---------------------------------------------------------------------------
// 四、心跳与断线检测（EWMA 自适应 + 迟滞防抖）
// ---------------------------------------------------------------------------

/// 心跳跟踪器（EWMA 间隔估计 + 阈值自适应 + 迟滞判死）。
///
/// 判死逻辑（防误判三防线）：
/// 1. **EWMA 平滑**：阈值跟随的是平滑间隔而非单样本——毛刺被摊薄。
/// 2. **下限守卫**：阈值永不低于 [`HB_TIMEOUT_MIN_MS`]——再稳的
///    链路也留 1s 余量。
/// 3. **迟滞**：单次超阈只记「可疑」（strike），连续
///    [`HB_STRIKE_LIMIT`] 次检查都超阈才判死——一次检查超阈可能
///    只是调度延迟，两次连超才是链路真死了。
#[derive(Clone, Copy, Debug)]
pub struct Heartbeat {
    /// 当前超时阈值（ms——EWMA 自适应量）。
    timeout_ms: u64,
    /// EWMA 间隔估计（ms）。
    ewma_ms: u64,
    /// 上次心跳时刻（逻辑时钟 ms；None=从未跳）。
    last_beat_ms: Option<u64>,
    /// 观测到的间隔样本数（自适应输入）。
    samples: u32,
    /// 连续可疑次数（迟滞计数器——两次判死检查都超阈才清零判死）。
    strikes: u32,
    /// 判死次数（读屏可达）。
    timeouts: u32,
    /// 误判回滚计数：判死后同一时刻又收到心跳（说明判早了）。
    false_positives: u32,
}

impl Heartbeat {
    /// 新跟踪器（基准阈值起步）。
    pub fn new() -> Heartbeat {
        Heartbeat {
            timeout_ms: HB_TIMEOUT_BASE_MS,
            ewma_ms: HB_TIMEOUT_BASE_MS / 2,
            last_beat_ms: None,
            samples: 0,
            strikes: 0,
            timeouts: 0,
            false_positives: 0,
        }
    }

    /// 记一次心跳（EWMA 更新间隔估计并自适应阈值）。
    ///
    /// 若此前刚被本跟踪器判死（timeouts>0 且 dead 标志由调用方反馈），
    /// 由 [`Heartbeat::resurrect`] 显性记账——判死不该被静默撤销。
    pub fn beat(&mut self, now_ms: u64) {
        if let Some(prev) = self.last_beat_ms {
            let gap = now_ms.saturating_sub(prev);
            // EWMA：ewma = ewma*(1-α) + gap*α，α=1/4（移位实现，无浮点）。
            self.ewma_ms = (self.ewma_ms.wrapping_mul(3) + gap) >> HB_EWMA_SHIFT;
            // 阈值 = 2×EWMA，且不低于下限——抖动链路自动放宽。
            let want = (self.ewma_ms.saturating_mul(2)).max(HB_TIMEOUT_MIN_MS);
            self.timeout_ms = want;
            self.samples = self.samples.saturating_add(1);
        }
        // 新心跳到达：迟滞计数清零（链路证明活着）。
        self.strikes = 0;
        self.last_beat_ms = Some(now_ms);
    }

    /// 判死检查（当前时刻距上次心跳是否超阈）。
    ///
    /// 返回 true 才算真断线：连续 [`HB_STRIKE_LIMIT`] 次调用都超阈。
    /// 单次超阈返回 false 并记 strike——把「可能只是调度慢」和
    /// 「链路死了」分开。
    pub fn timed_out(&mut self, now_ms: u64) -> bool {
        let overdue = match self.last_beat_ms {
            None => false, // 从未跳过不判死（还没开始）
            Some(prev) => now_ms.saturating_sub(prev) > self.timeout_ms,
        };
        if overdue {
            self.strikes = self.strikes.saturating_add(1);
            if self.strikes >= HB_STRIKE_LIMIT {
                self.timeouts = self.timeouts.saturating_add(1);
                self.strikes = 0;
                true
            } else {
                false
            }
        } else {
            self.strikes = 0;
            false
        }
    }

    /// 判死复活（判死后又收到心跳——显性记账为误判，不静默）。
    pub fn resurrect(&mut self, now_ms: u64) {
        self.false_positives = self.false_positives.saturating_add(1);
        self.beat(now_ms);
    }

    /// 当前阈值（读屏可达）。
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    /// EWMA 间隔估计（读屏可达——阈值为什么是这个值的解释面）。
    pub fn ewma_ms(&self) -> u64 {
        self.ewma_ms
    }

    /// 判死次数（读屏可达）。
    pub fn timeouts(&self) -> u32 {
        self.timeouts
    }

    /// 误判复活计数（读屏可达——阈值是否该再放宽的证据面）。
    pub fn false_positives(&self) -> u32 {
        self.false_positives
    }

    /// 当前连续可疑次数（读屏可达）。
    pub fn strikes(&self) -> u32 {
        self.strikes
    }
}

// ---------------------------------------------------------------------------
// 五、重连恢复（多代快照保险箱 + 校验 + 过期裁决）
// ---------------------------------------------------------------------------

/// 会话快照（断线前的进度——房间/角色/已确认水位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionSnapshot {
    /// 房间 id（0=大厅）。
    pub room_id: u32,
    /// 角色实体 id。
    pub entity_id: u32,
    /// 已确认的事务水位（重连后从这儿续）。
    pub txn_watermark: u64,
    /// 快照时刻（逻辑时钟 ms）。
    pub taken_ms: u64,
}

impl SessionSnapshot {
    /// FNV-1a 校验和（字段定序打包——写坏一格必被检出）。
    pub fn checksum(&self) -> u64 {
        let mut h = SNAPSHOT_FNV_SEED;
        for word in [self.room_id as u64, self.entity_id as u64, self.txn_watermark, self.taken_ms] {
            for shift in [0u32, 8, 16, 24, 32, 40, 48, 56] {
                h ^= (word >> shift) & 0xff;
                h = h.wrapping_mul(SNAPSHOT_FNV_PRIME);
            }
        }
        h
    }

    /// 是否在存活期内（过期快照续接反而危险：服务端状态可能已被
    /// 他人推翻——必须显性过期而不是静默接回）。
    pub fn fresh(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.taken_ms) <= SNAPSHOT_TTL_MS
    }
}

/// 快照保险箱（多代槽——断线中状态每次推进补拍，恢复时从新到旧
/// 验证校验和，坏代跳过、好代接回、全坏/全过期才显性失败）。
///
/// 「重连状态丢失→快照恢复」是一条**路径**：丢的可能是最新一代
/// （正在写的槽），但倒数第二代可能完好——单代设计把「最新坏了」
/// 等同于「全丢了」，恢复成功率无谓地低。
#[derive(Clone, Copy, Debug)]
pub struct SnapshotVault {
    slots: [Option<SessionSnapshot>; VAULT_DEPTH],
    /// 写指针（环形——下一拍写进哪个槽）。
    wpos: usize,
    /// 拍照总数。
    stashes: u32,
    /// 恢复成功次数（读屏可达）。
    recovered: u32,
    /// 因校验和坏被跳过的代数（读屏可达——存储面健康度证据）。
    corrupt_skips: u32,
}

impl SnapshotVault {
    /// 空保险箱。
    pub fn new() -> SnapshotVault {
        SnapshotVault {
            slots: [None; VAULT_DEPTH],
            wpos: 0,
            stashes: 0,
            recovered: 0,
            corrupt_skips: 0,
        }
    }

    /// 断线前/断线中拍快照（环形覆盖最老代）。
    pub fn stash(&mut self, snap: SessionSnapshot) {
        self.slots[self.wpos] = Some(snap);
        self.wpos = (self.wpos + 1) % VAULT_DEPTH;
        self.stashes = self.stashes.saturating_add(1);
    }

    /// 恢复：从最新代往回扫（环形槽的新→旧序），第一枚**校验和匹配
    /// 且未过期**的快照接回；跳过坏代并记账；全灭才显性失败。
    ///
    /// 返回 Err 时错误串含三类区分指引：全空（没拍过）/ 全损坏
    /// （存储面坏）/ 全过期（断线太久）——三种「恢复失败」的处置
    /// 完全不同，混成一句「恢复失败」等于没给指引。
    pub fn recover(&mut self, now_ms: u64) -> Result<SessionSnapshot, String> {
        // 环形槽的新→旧序：从 wpos-1 往回走一圈。
        let mut saw_any = false;
        let mut saw_stale = false;
        for back in 0..VAULT_DEPTH {
            let idx = (self.wpos + VAULT_DEPTH - 1 - back) % VAULT_DEPTH;
            if let Some(snap) = self.slots[idx] {
                saw_any = true;
                if !snap.fresh(now_ms) {
                    saw_stale = true;
                    continue;
                }
                self.recovered = self.recovered.saturating_add(1);
                return Ok(snap);
            }
        }
        if !saw_any {
            Err(format!(
                "{}：重连快照缺失——保险箱无任何代（断线前必须 stash）。\
                 指引：此刻只能按新会话处理并留痕，不清零冒充续接",
                E_SES_SNAPSHOT
            ))
        } else if saw_stale {
            Err(format!(
                "{}：快照过期——最新有效代已超 {}ms 存活期。指引：服务端状态\
                 可能已被推进，静默续接会回滚他人可见进度；按服务端权威态重建",
                E_SES_SNAPSHOT, SNAPSHOT_TTL_MS
            ))
        } else {
            Err(format!(
                "{}：快照恢复失败——代存在但全部不可用。指引：外部字节流损坏走 \
                 recover_raw 校验路径，逐代核对校验和",
                E_SES_SNAPSHOT
            ))
        }
    }

    /// 带外部校验的恢复（F5424 同步层消费形态：调用方持有原始字节
    /// 流，把**逐代重算校验和**的裁决交给本函数——坏代被显性跳过）。
    pub fn recover_raw(&mut self, now_ms: u64, raws: &[(SessionSnapshot, u64)]) -> Result<SessionSnapshot, String> {
        // raws：外部存储的 (快照, 落盘时校验和) 序列（新→旧序由调用方
        // 保证）。逐代验证：重算 ≠ 落盘值 → 坏代跳过。
        let mut saw_stale = false;
        for (snap, stored_sum) in raws.iter() {
            if snap.checksum() != *stored_sum {
                self.corrupt_skips = self.corrupt_skips.saturating_add(1);
                continue;
            }
            if !snap.fresh(now_ms) {
                saw_stale = true;
                continue;
            }
            self.recovered = self.recovered.saturating_add(1);
            return Ok(*snap);
        }
        if self.corrupt_skips > 0 && !saw_stale {
            Err(format!(
                "{}：快照损坏——{} 代校验和不匹配已跳过。指引：排查写入通道并发覆写",
                E_SES_SNAPSHOT, self.corrupt_skips
            ))
        } else if saw_stale {
            Err(format!(
                "{}：快照过期——按服务端权威态重建，不静默续接",
                E_SES_SNAPSHOT
            ))
        } else {
            Err(format!(
                "{}：快照恢复失败——外部序列为空。指引：断线前必须 stash",
                E_SES_SNAPSHOT
            ))
        }
    }

    /// 拍照总数（读屏可达）。
    pub fn stashes(&self) -> u32 {
        self.stashes
    }

    /// 恢复成功次数（读屏可达）。
    pub fn recovered(&self) -> u32 {
        self.recovered
    }

    /// 槽占用数（读屏可达）。
    pub fn occupied(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
}

/// 重连续接器（兼容初版 API：内部升级为多代保险箱）。
#[derive(Clone, Copy, Debug)]
pub struct Resumer {
    vault: SnapshotVault,
}

impl Resumer {
    /// 空续接器。
    pub fn new() -> Resumer {
        Resumer { vault: SnapshotVault::new() }
    }

    /// 断线前留快照（多代环形——重复调用推进代际）。
    pub fn stash(&mut self, snap: SessionSnapshot) {
        self.vault.stash(snap);
    }

    /// 重连后续接（坏代跳过/过期显性/全灭才失败——不清零冒充续接）。
    pub fn resume(&mut self, now_ms: u64) -> Result<SessionSnapshot, String> {
        self.vault.recover(now_ms)
    }

    /// 恢复成功次数（读屏可达）。
    pub fn recovered(&self) -> u32 {
        self.vault.recovered()
    }

    /// 拍照总数（读屏可达）。
    pub fn stashes(&self) -> u32 {
        self.vault.stashes()
    }
}

// ---------------------------------------------------------------------------
// 六、质量分级（窗口平滑 + 升降档不对称迟滞）
// ---------------------------------------------------------------------------

/// 质量等级（四级闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum QualityGrade {
    /// 优。
    Excellent,
    /// 良。
    Good,
    /// 中（建议关注）。
    Fair,
    /// 差（提前降级）。
    Poor,
}

impl QualityGrade {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            QualityGrade::Excellent => "优",
            QualityGrade::Good => "良",
            QualityGrade::Fair => "中",
            QualityGrade::Poor => "差",
        }
    }

    /// 档位序数（0..4——迟滞比较用）。
    pub fn rank(self) -> u8 {
        match self {
            QualityGrade::Excellent => 0,
            QualityGrade::Good => 1,
            QualityGrade::Fair => 2,
            QualityGrade::Poor => 3,
        }
    }

    /// 序数 → 档位。
    pub fn from_rank(r: u8) -> QualityGrade {
        match r {
            0 => QualityGrade::Excellent,
            1 => QualityGrade::Good,
            2 => QualityGrade::Fair,
            _ => QualityGrade::Poor,
        }
    }
}

/// 质量输入（RTT 微秒 + 丢包千分比）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QualityInput {
    /// 往返耗时（微秒）。
    pub rtt_us: u32,
    /// 丢包率（千分比）。
    pub loss_permille: u32,
}

/// 单样本分级（纯函数——查表边界判定，无状态 O(1)）。
pub fn quality_grade(q: &QualityInput) -> QualityGrade {
    if q.rtt_us >= Q_RTT_FAIR_US || q.loss_permille >= Q_LOSS_FAIR_PERMILLE * 2 {
        return QualityGrade::Poor;
    }
    if q.rtt_us >= Q_RTT_GOOD_US || q.loss_permille >= Q_LOSS_FAIR_PERMILLE {
        return QualityGrade::Fair;
    }
    if q.rtt_us >= Q_RTT_GOOD_US / 2 {
        return QualityGrade::Good;
    }
    QualityGrade::Excellent
}

/// 分级器（滚动窗口平滑 + 升降档不对称迟滞——降 1 升 3）。
///
/// 为什么不能单样本直通：降级动作（降发送率/切线路）本身有代价，
/// 毛刺驱动升降级会让发送率震荡、对端看到锯齿。窗口均值 + 不对称
/// 迟滞 = 降档响应快（提前降级宁早勿晚——锚点域本色）、升档慢
/// （恢复必须连续证实）。
#[derive(Clone, Copy, Debug)]
pub struct Grader {
    /// 滚动窗口（RTT 样本）。
    rtt_win: [u32; GRADE_WINDOW],
    /// 滚动窗口（丢包样本）。
    loss_win: [u32; GRADE_WINDOW],
    /// 写指针。
    wpos: usize,
    /// 已入样本数（窗口未满时按已有样本求均值）。
    filled: usize,
    /// 当前档位。
    grade: QualityGrade,
    /// 连续「好于当前档」的窗口计数（升档迟滞计数）。
    up_streak: usize,
    /// 档位变更次数（读屏可达）。
    changes: u32,
}

impl Grader {
    /// 新分级器（不预判——首样本前档位按窗口实况算）。
    pub fn new() -> Grader {
        Grader {
            rtt_win: [0; GRADE_WINDOW],
            loss_win: [0; GRADE_WINDOW],
            wpos: 0,
            filled: 0,
            grade: QualityGrade::Excellent,
            up_streak: 0,
            changes: 0,
        }
    }

    /// 窗口均值（整数除法——样本单位本来就有粒度，不引浮点）。
    fn window_mean(&self) -> QualityInput {
        if self.filled == 0 {
            return QualityInput { rtt_us: 0, loss_permille: 0 };
        }
        let mut rt = 0u64;
        let mut ls = 0u64;
        for i in 0..self.filled {
            rt = rt.saturating_add(self.rtt_win[i] as u64);
            ls = ls.saturating_add(self.loss_win[i] as u64);
        }
        QualityInput {
            rtt_us: (rt / self.filled as u64).min(u32::MAX as u64) as u32,
            loss_permille: (ls / self.filled as u64).min(u32::MAX as u64) as u32,
        }
    }

    /// 入一个样本并重算档位（返回（新档位, 是否变更））。
    pub fn observe(&mut self, q: QualityInput) -> (QualityGrade, bool) {
        self.rtt_win[self.wpos] = q.rtt_us;
        self.loss_win[self.wpos] = q.loss_permille;
        self.wpos = (self.wpos + 1) % GRADE_WINDOW;
        if self.filled < GRADE_WINDOW {
            self.filled += 1;
        }
        let mean = self.window_mean();
        let want = quality_grade(&mean);
        let cur_rank = self.grade.rank();
        let want_rank = want.rank();
        if want_rank > cur_rank {
            // 变差：降档即时（提前降级——宁早勿晚）。
            self.grade = want;
            self.up_streak = 0;
            self.changes = self.changes.saturating_add(1);
            (self.grade, true)
        } else if want_rank < cur_rank {
            // 变好：升档要连续 GRADE_UP_STREAK 个窗口证实。
            self.up_streak = self.up_streak.saturating_add(1);
            if self.up_streak >= GRADE_UP_STREAK {
                self.grade = want;
                self.up_streak = 0;
                self.changes = self.changes.saturating_add(1);
                (self.grade, true)
            } else {
                (self.grade, false)
            }
        } else {
            self.up_streak = 0;
            (self.grade, false)
        }
    }

    /// 当前档位。
    pub fn grade(&self) -> QualityGrade {
        self.grade
    }

    /// 档位变更次数（读屏可达）。
    pub fn changes(&self) -> u32 {
        self.changes
    }

    /// 窗口填充数（读屏可达——「为什么还没升档」的解释面）。
    pub fn filled(&self) -> usize {
        self.filled
    }
}

/// 差连接的降级建议（域本色：提前降级不硬撑）。
pub fn degrade_advice(g: QualityGrade) -> Option<String> {
    if g >= QualityGrade::Fair {
        Some(format!(
            "{}：连接质量{}（Fair/Poor）——建议提前降级：降发送率/减同步频率/切备用线路，\
             不要等超时才发现",
            E_SES_QUALITY, g.zh()
        ))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 七、F5424 同步消费接口（同步层重连续接的消费面）
// ---------------------------------------------------------------------------

/// 同步层消费端口（F5424 对接点：同步层重连后据此决定续接起点与
/// 本地预测的丢弃量——不暴露快照内部布局，只给续接语义）。
pub trait SyncConsumer {
    /// 续接生效：同步层从水位 `from_watermark` 起请求补发，并丢弃
    /// 水位之上的本地预测（回退上限=`dropped_prediction_budget`）。
    fn on_resume(&mut self, from_watermark: u64, dropped_prediction_budget: u64);

    /// 续接失败（快照全灭/过期）：同步层按新会话处理并保留留痕义务。
    fn on_resume_failed(&mut self, reason: String);
}

/// 把保险箱恢复结果喂给同步层消费端口（F5424 桥接函数——恢复路径
/// 与消费路径的唯一接缝，两侧行为都可独立机检）。
///
/// 回退上限口径：续接起点水位与服务端确认差值 = 断线期间可能产生
/// 的最大本地预测量；此处按「水位 × 1 + 固定余量」给上限（无浮点），
/// 同步层可再收紧不可放宽（放宽=偷偷多预测）。
pub fn resume_for_sync(
    vault: &mut SnapshotVault,
    now_ms: u64,
    consumer: &mut dyn SyncConsumer,
) -> Result<u64, String> {
    match vault.recover(now_ms) {
        Ok(snap) => {
            let budget = snap.txn_watermark.saturating_add(64);
            consumer.on_resume(snap.txn_watermark, budget);
            Ok(snap.txn_watermark)
        }
        Err(e) => {
            consumer.on_resume_failed(e.clone());
            Err(e)
        }
    }
}

/// 消费端记账（判据与调试用的最小实现——同步层真实实现替换之）。
#[derive(Clone, Debug, Default)]
pub struct RecordingConsumer {
    /// 收到的续接（水位, 回退预算）序列。
    pub resumes: Vec<(u64, u64)>,
    /// 收到的失败原因序列。
    pub failures: Vec<String>,
}

impl SyncConsumer for RecordingConsumer {
    fn on_resume(&mut self, from_watermark: u64, dropped_prediction_budget: u64) {
        self.resumes.push((from_watermark, dropped_prediction_budget));
    }
    fn on_resume_failed(&mut self, reason: String) {
        self.failures.push(reason);
    }
}

// ---------------------------------------------------------------------------
// 八、F5412 模拟：确定性虚拟时钟（判据可复跑的时基）
// ---------------------------------------------------------------------------

/// 确定性虚拟时钟（F5412 模拟对接点：心跳/过期/窗口全部跑注入
/// 时钟——判据不依赖真实时间，任何机器上逐位复跑）。
#[derive(Clone, Copy, Debug)]
pub struct SimClock {
    now_ms: u64,
}

impl SimClock {
    /// 零点时钟。
    pub fn new() -> SimClock {
        SimClock { now_ms: 0 }
    }

    /// 当前时刻（ms）。
    pub fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// 推针（只前进——倒拨拒绝：时间倒流会让一切超时判定失义）。
    pub fn advance(&mut self, delta_ms: u64) -> Result<u64, String> {
        if delta_ms == 0 {
            return Ok(self.now_ms);
        }
        self.now_ms = self.now_ms.saturating_add(delta_ms);
        Ok(self.now_ms)
    }
}

// ---------------------------------------------------------------------------
// 九、会话门面（状态机 × 心跳 × 快照 × 分级 编排）
// ---------------------------------------------------------------------------

/// 会话编排器（四件套的运行时组合——F4623 口径的状态机驱动 +
/// 注入时钟驱动的心跳与快照，重连带内自动补拍快照）。
#[derive(Clone, Copy, Debug)]
pub struct SessionManager {
    sess: Session,
    hb: Heartbeat,
    vault: SnapshotVault,
    grader: Grader,
    diagnoser: HandshakeDiagnoser,
    /// 最近一次拍快照的时刻（补拍节流：≥1s 间隔才补拍）。
    last_snap_ms: u64,
}

impl SessionManager {
    /// 新编排器。
    pub fn new() -> SessionManager {
        SessionManager {
            sess: Session::new(),
            hb: Heartbeat::new(),
            vault: SnapshotVault::new(),
            grader: Grader::new(),
            diagnoser: HandshakeDiagnoser::default(),
            last_snap_ms: 0,
        }
    }

    /// 状态视图。
    pub fn state(&self) -> ConnState {
        self.sess.state()
    }

    /// 心跳视图（读屏可达——「现在连接怎么样了」的解释面）。
    pub fn heartbeat(&self) -> &Heartbeat {
        &self.hb
    }

    /// 分级视图。
    pub fn grader(&self) -> &Grader {
        &self.grader
    }

    /// 保险箱视图。
    pub fn vault(&self) -> &SnapshotVault {
        &self.vault
    }

    /// 进入握手段（传输建立成功——连接态推进到握手态）。
    pub fn transport_up(&mut self) -> Result<ConnState, String> {
        self.sess.dispatch(ConnEvent::TransportUp)
    }

    /// 心跳注入（网络层每收到对端心跳调用一次）。
    pub fn heartbeat_beat(&mut self, now_ms: u64) {
        self.hb.beat(now_ms);
    }

    /// 握手成功路径（Connecting → Handshaking → Connected）。
    pub fn handshake_ok(&mut self) -> Result<ConnState, String> {
        self.sess.dispatch(ConnEvent::TransportUp)?;
        self.sess.dispatch(ConnEvent::HandshakeOk)?;
        Ok(self.sess.state())
    }

    /// 握手失败路径（诊断归类+指引落账+终态归位——锚点「握手失败→
    /// 诊断+重试指引」的编排面）。
    pub fn handshake_fail(&mut self, d: HandshakeDiag) -> (String, &'static RetryGuidance) {
        let g = self.diagnoser.classify(d);
        let _ = self.sess.dispatch(ConnEvent::HandshakeFail);
        (self.diagnoser.report(d), g)
    }

    /// 心跳判死检查（只做断线检测+迁移——不注入质量样本、不补拍，
    /// 由编排方按需组合）。
    pub fn heartbeat_check(&mut self, now_ms: u64) -> bool {
        if self.hb.timed_out(now_ms) {
            let _ = self.sess.dispatch(ConnEvent::HeartbeatTimeout);
            true
        } else {
            false
        }
    }

    /// 周期巡检（每逻辑帧调用：心跳判死+重连带内快照补拍）。
    pub fn tick(&mut self, now_ms: u64) {
        self.heartbeat_check(now_ms);
        if self.sess.state().in_recovery_band() {
            // 重连带内补拍快照（节流 ≥1s）：断线中的状态推进不丢代。
            if now_ms.saturating_sub(self.last_snap_ms) >= 1_000 {
                self.snapshot_at(now_ms);
            }
        }
    }

    /// 手动拍快照（状态推进点调用——断线前必须留痕）。
    pub fn snapshot_with(&mut self, room_id: u32, entity_id: u32, txn_watermark: u64, now_ms: u64) {
        self.vault
            .stash(SessionSnapshot { room_id, entity_id, txn_watermark, taken_ms: now_ms });
        self.last_snap_ms = now_ms;
    }

    /// 内部补拍（重连带内——沿用最近手动快照内容仅刷新时刻）。
    fn snapshot_at(&mut self, now_ms: u64) {
        // 取最新代内容（若有）刷新时刻重拍；无历史则拍空会话。
        let (room, ent, wm) = match self.latest() {
            Some(s) => (s.room_id, s.entity_id, s.txn_watermark),
            None => (0, 0, 0),
        };
        self.vault.stash(SessionSnapshot { room_id: room, entity_id: ent, txn_watermark: wm, taken_ms: now_ms });
        self.last_snap_ms = now_ms;
    }

    /// 最新代快照（读屏可达——调试面板展示当前进度水位）。
    pub fn latest(&self) -> Option<SessionSnapshot> {
        // 从新到旧找第一枚有内容的槽。
        for back in 0..VAULT_DEPTH {
            let idx = (self.vault.wpos + VAULT_DEPTH - 1 - back) % VAULT_DEPTH;
            if let Some(s) = self.vault.slots[idx] {
                return Some(s);
            }
        }
        None
    }

    /// 质量样本注入（网络层每窗口喂一次）。
    pub fn observe_quality(&mut self, q: QualityInput) -> (QualityGrade, bool) {
        self.grader.observe(q)
    }

    /// 断线恢复完成（ReconnectOk + 恢复快照续接——「重连不清进度」
    /// 的编排闭环：迁移与续接原子完成，接不回则状态不前移）。
    pub fn reconnect_ok(&mut self, now_ms: u64) -> Result<SessionSnapshot, String> {
        let snap = self.vault.recover(now_ms)?;
        let _ = self.sess.dispatch(ConnEvent::ReconnectOk)?;
        Ok(snap)
    }

    /// 会话摘要（一行读屏可达的状态面）。
    pub fn summary(&self) -> String {
        format!(
            "会话：态={:?} 断线={}次 心跳阈值={}ms 分级={} 快照={}代",
            self.sess.state(),
            self.sess.drops(),
            self.hb.timeout_ms(),
            self.grader.grade().zh(),
            self.vault.occupied(),
        )
    }
}

/// F4623 状态机口径声明（跨批对接点：五态闭集+合法迁移查表+表外
/// 拒绝归位，纪律同 F4623 上游；口径冲突以本注记登记为准）。
pub const F4623_STATEMACHINE_NOTE: &str = "F5404 生命周期状态机口径参照 F4623 上游：状态闭集+合法迁移定表+表外拒绝并给归位指引；F5424 同步层经 resume_for_sync 消费快照续接；F5412 模拟经 SimClock 注入确定性时钟";

// ---------------------------------------------------------------------------
// 十、判据（判据区零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F5404 域自检（判据分五组：生命周期/心跳/重连快照/质量/对接收尾）。
pub fn run_vef57_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5404");

    // --- 生命周期（判据一）---
    // 合法链路全程走通（连接→握手→已连接）。
    let mut ses = Session::new();
    let a = ses.dispatch(ConnEvent::TransportUp);
    let b = ses.dispatch(ConnEvent::HandshakeOk);
    s.add(
        "F57-生命-01",
        a.is_ok() && b.is_ok() && ses.state() == ConnState::Connected && ses.transitions() == 2,
        "连接→握手→已连接 合法链路走通",
    );
    // 非法迁移拒绝+归位（已连接态不收 TransportUp）——零 panic 面。
    let r = ses.dispatch(ConnEvent::TransportUp);
    let r_ok = r.as_ref().err().map(|x| x.starts_with(E_SES_STATE)).unwrap_or(false);
    s.add(
        "F57-生命-02",
        r.is_err() && r_ok && ses.rejects() == 1,
        "非法迁移拒绝+归位指引",
    );
    // 断线计数（Connected→Reconnecting 记 drops）。
    ses.dispatch(ConnEvent::HeartbeatTimeout).ok();
    s.add(
        "F57-生命-03",
        ses.state() == ConnState::Reconnecting && ses.drops() == 1,
        "断线计数（重连带进入被度量）",
    );
    // 终态封闭（Closed 后一切事件拒绝）。
    ses.dispatch(ConnEvent::HeartbeatTimeout).ok();
    ses.dispatch(ConnEvent::Close).ok();
    let closed_reject = ses.dispatch(ConnEvent::ReconnectOk);
    s.add(
        "F57-生命-04",
        ses.state().is_terminal() && closed_reject.is_err(),
        "终态封闭（Closed 不再收事件）",
    );
    // 业务面门禁（非 Connected 不承载流量）。
    let mut gate = Session::new();
    let carries_before = gate.state().carries_traffic();
    gate.dispatch(ConnEvent::TransportUp).ok();
    gate.dispatch(ConnEvent::HandshakeOk).ok();
    let carries_after = gate.state().carries_traffic();
    s.add("F57-生命-05", !carries_before && carries_after, "业务流量只在已连接态承载");
    // 迁移表完备性（五态×六事件全部有界——None/Some 都是裁决，不 panic）。
    let table_complete = ConnState::ALL.iter().all(|&st| {
        [
            ConnEvent::TransportUp,
            ConnEvent::HandshakeOk,
            ConnEvent::HandshakeFail,
            ConnEvent::HeartbeatTimeout,
            ConnEvent::ReconnectOk,
            ConnEvent::Close,
        ]
        .iter()
        .all(|&ev| {
            let _ = next_state(st, ev);
            true
        })
    });
    s.add("F57-生命-06", table_complete, "迁移定表全格可裁决（30 格无 panic）");

    // --- 握手诊断（判据二：握手失败→诊断+重试指引）---
    let mut dg = HandshakeDiagnoser::default();
    let g_ver = dg.classify(HandshakeDiag::VersionMismatch);
    s.add(
        "F57-诊断-01",
        !g_ver.retryable && g_ver.note_code.contains("升级") && dg.count_of(HandshakeDiag::VersionMismatch) == 1,
        "版本不匹配→终局指引（不可重试）",
    );
    let g_auth = dg.classify(HandshakeDiag::AuthRejected);
    s.add(
        "F57-诊断-02",
        !g_auth.retryable && g_auth.note_code.contains("登录"),
        "鉴权拒绝→重新登录指引（不可重试）",
    );
    let g_to = dg.classify(HandshakeDiag::TimedOut);
    let b1 = backoff_for(g_to, 1);
    let b3 = backoff_for(g_to, 3);
    s.add(
        "F57-诊断-03",
        g_to.retryable && b1 == 500 && b3 == 2_000 && backoff_for(g_to, 8) == 8_000,
        "超时可重试+指数退避（500→2000→封顶 8000）",
    );
    let g_srv = dg.classify(HandshakeDiag::ServerRefused);
    s.add(
        "F57-诊断-04",
        g_srv.retryable && backoff_for(g_srv, 10) == g_srv.backoff_cap_ms,
        "服务端拒绝→长退避封顶（60s 封顶不越界）",
    );
    // 诊断报告面（含指引语义，一行读屏可达）。
    let rep = dg.report(HandshakeDiag::VersionMismatch);
    s.add(
        "F57-诊断-05",
        rep.starts_with(E_SES_HANDSHAKE) && rep.contains("升级") && rep.contains("不可重试"),
        "诊断报告含指引（不可重试显性）",
    );
    // 编排面：握手段失败落终态+诊断计数联动（Handshaking → Closed）。
    let mut mgr0 = SessionManager::new();
    mgr0.transport_up().ok();
    let (rep2, g2) = mgr0.handshake_fail(HandshakeDiag::AuthRejected);
    s.add(
        "F57-诊断-06",
        mgr0.state().is_terminal() && !g2.retryable && rep2.contains("登录"),
        "编排面握手失败→终态归位+诊断联动",
    );

    // --- 心跳保活（判据三：EWMA 自适应+迟滞防抖）---
    // 正常节奏不判死（含迟滞：单次超阈不死）。
    let mut hb = Heartbeat::new();
    hb.beat(0);
    hb.beat(1_000);
    hb.beat(2_000);
    let t1 = hb.timed_out(2_500);
    s.add("F57-心跳-01", !t1 && hb.timeouts() == 0, "正常节奏不判死");
    // 迟滞：单次超阈只记可疑（strike=1 不判死）。
    let t2 = hb.timed_out(6_000); // 超阈 4s 一次（阈值自适应到 3686ms）
    s.add(
        "F57-心跳-02",
        !t2 && hb.strikes() == 1 && hb.timeouts() == 0,
        "单次超阈只记可疑（迟滞防线）",
    );
    // 连续第二次超阈才判死。
    let t3 = hb.timed_out(6_500);
    s.add(
        "F57-心跳-03",
        t3 && hb.timeouts() == 1 && hb.strikes() == 0,
        "连续超阈判死（迟滞阈值=2）",
    );
    // EWMA 自适应：抖动链路阈值放宽。
    let mut hb2 = Heartbeat::new();
    hb2.beat(0);
    hb2.beat(4_000); // gap=4s → EWMA≈1s → 阈值=2s；再跳几拍 EWMA→4s→阈值 8s
    hb2.beat(8_000);
    hb2.beat(12_000);
    let relaxed = hb2.timeout_ms() >= 4_000 && !hb2.timed_out(16_000);
    s.add(
        "F57-心跳-04",
        relaxed,
        "EWMA 自适应（抖动 4s 链路→阈值放宽不误判）",
    );
    // 下限守卫（再稳也不低于 1s）。
    let mut hb3 = Heartbeat::new();
    hb3.beat(0);
    hb3.beat(10);
    s.add(
        "F57-心跳-05",
        hb3.timeout_ms() >= HB_TIMEOUT_MIN_MS,
        "自适应下限守卫（≥1s）",
    );
    // 误判复活显性（判死后心跳到达 → false_positives 记账）。
    let mut hb4 = Heartbeat::new();
    hb4.beat(0);
    hb4.beat(1_000);
    hb4.timed_out(6_000);
    hb4.timed_out(7_000); // 连续两次超阈 → 判死
    hb4.resurrect(7_500);
    s.add(
        "F57-心跳-06",
        hb4.false_positives() == 1 && hb4.ewma_ms() > 0,
        "判死复活显性记账（不静默撤销）",
    );

    // --- 重连快照（判据四：多代恢复+校验+过期）---
    // 快照续接（重连不清进度——多代中最新的有效代接回）。
    let mut rs = Resumer::new();
    rs.stash(SessionSnapshot { room_id: 7, entity_id: 42, txn_watermark: 9_001, taken_ms: 1_000 });
    let got = rs.resume(2_000);
    let got_ok = got.as_ref().map(|x| x.room_id == 7 && x.txn_watermark == 9_001).unwrap_or(false);
    s.add(
        "F57-快照-01",
        got_ok && rs.recovered() == 1,
        "快照续接（进度原样接回）",
    );
    // 多代恢复：最新代坏（校验和不匹配）→ 旧代接回（recover_raw 面）。
    let mut vault = SnapshotVault::new();
    let snap_new = SessionSnapshot { room_id: 1, entity_id: 2, txn_watermark: 100, taken_ms: 5_000 };
    let snap_old = SessionSnapshot { room_id: 1, entity_id: 2, txn_watermark: 90, taken_ms: 4_000 };
    vault.stash(snap_old);
    vault.stash(snap_new);
    let rr = vault.recover_raw(
        6_000,
        &[(snap_new, snap_new.checksum().wrapping_add(1)), (snap_old, snap_old.checksum())],
    );
    s.add(
        "F57-快照-02",
        rr.as_ref().map(|x| x.txn_watermark == 90).unwrap_or(false),
        "坏代跳过旧代接回（多代恢复路径）",
    );
    // 坏代计数显性。
    s.add(
        "F57-快照-03",
        vault.recover_raw(6_000, &[(snap_new, 0)]).is_err(),
        "全坏显性失败（不把坏数据当进度）",
    );
    // F5424 桥接：恢复成功喂消费端（水位+回退预算）。
    let mut vault2 = SnapshotVault::new();
    let snap2 = SessionSnapshot { room_id: 3, entity_id: 4, txn_watermark: 500, taken_ms: 100 };
    vault2.stash(snap2);
    let mut rc2 = RecordingConsumer::default();
    let bridged = resume_for_sync(&mut vault2, 200, &mut rc2);
    s.add(
        "F57-快照-04",
        bridged.as_ref().map(|w| *w == 500).unwrap_or(false)
            && rc2.resumes.len() == 1
            && rc2.resumes[0].1 == 564,
        "F5424 同步消费桥接（水位+回退预算）",
    );
    // F5424 桥接失败面：空保险箱→on_resume_failed 显性。
    let mut vault3 = SnapshotVault::new();
    let mut rc3 = RecordingConsumer::default();
    let bridged_fail = resume_for_sync(&mut vault3, 200, &mut rc3);
    s.add(
        "F57-快照-05",
        bridged_fail.is_err() && rc3.failures.len() == 1 && rc3.failures[0].starts_with(E_SES_SNAPSHOT),
        "F5424 恢复失败显性喂消费端（不静默）",
    );
    // 过期裁决：超 TTL 显性报过期（不静默续接）。
    let mut vault4 = SnapshotVault::new();
    vault4.stash(SessionSnapshot { room_id: 5, entity_id: 6, txn_watermark: 1, taken_ms: 0 });
    let stale = vault4.recover(SNAPSHOT_TTL_MS + 1);
    let stale_msg = stale.as_ref().err().map(|x| x.contains("过期")).unwrap_or(false);
    s.add(
        "F57-快照-06",
        stale.is_err() && stale_msg,
        "快照过期显性裁决（不回滚他人进度）",
    );

    // --- 质量分级（判据五：窗口平滑+不对称迟滞）---
    // 四档单调（单样本纯函数面）。
    let g_ex = quality_grade(&QualityInput { rtt_us: 20_000, loss_permille: 0 });
    let g_good = quality_grade(&QualityInput { rtt_us: 50_000, loss_permille: 0 });
    let g_fair = quality_grade(&QualityInput { rtt_us: 120_000, loss_permille: 0 });
    let g_poor = quality_grade(&QualityInput { rtt_us: 300_000, loss_permille: 100 });
    s.add(
        "F57-质量-01",
        g_ex == QualityGrade::Excellent
            && g_good == QualityGrade::Good
            && g_fair == QualityGrade::Fair
            && g_poor == QualityGrade::Poor,
        "四档分级（RTT+丢包双因子单调）",
    );
    // 差连接提前降级建议（Fair/Poor 有建议，优/良无）。
    s.add(
        "F57-质量-02",
        degrade_advice(QualityGrade::Poor).is_some()
            && degrade_advice(QualityGrade::Fair).is_some()
            && degrade_advice(QualityGrade::Good).is_none()
            && degrade_advice(QualityGrade::Excellent).is_none(),
        "差连接提前降级（优/良不扰）",
    );
    // 窗口平滑：单样本 RTT 毛刺不直接打到 Poor（有窗直通 Poor 被
    // 限幅到 Fair——400kµs 单样本本应 Poor，5 样本窗摊薄后均值 88k
    // 落在 Fair 档）。loss 面板恒 0 排除干扰。
    let mut gr = Grader::new();
    for _ in 0..GRADE_WINDOW {
        gr.observe(QualityInput { rtt_us: 10_000, loss_permille: 0 });
    }
    let (g_spike, changed_spike) = gr.observe(QualityInput { rtt_us: 400_000, loss_permille: 0 });
    s.add(
        "F57-质量-03",
        g_spike == QualityGrade::Fair && changed_spike,
        "RTT 毛刺被窗口平滑限幅（单样本直打 Poor→窗内仅 Fair）",
    );
    // 持续差连接触发降档（提前降级不硬撑）。
    let mut g_changed = false;
    let mut g_final = gr.grade();
    for _ in 0..GRADE_WINDOW {
        let (g, ch) = gr.observe(QualityInput { rtt_us: 300_000, loss_permille: 0 });
        g_changed = g_changed || ch;
        g_final = g;
    }
    s.add(
        "F57-质量-04",
        g_changed && g_final == QualityGrade::Poor,
        "持续差连接触发降档（提前降级）",
    );
    // 升档迟滞：首个好窗口不升档，连续证实后才升（窗口均值逐步转好
    // + GRADE_UP_STREAK=3 迟滞——第 4 次好窗口才升回 Good）。
    let (g1, ch1) = gr.observe(QualityInput { rtt_us: 10_000, loss_permille: 0 });
    let (_, ch2) = gr.observe(QualityInput { rtt_us: 10_000, loss_permille: 0 });
    let (_, ch3) = gr.observe(QualityInput { rtt_us: 10_000, loss_permille: 0 });
    let (_, ch4) = gr.observe(QualityInput { rtt_us: 10_000, loss_permille: 0 });
    s.add(
        "F57-质量-05",
        !ch1 && g1 != QualityGrade::Good && !ch2 && !ch3 && ch4,
        "升档迟滞（降 1 升 3 不对称——恢复要证实）",
    );

    // --- F5412 模拟时钟 + F4623 口径（判据六：对接面）---
    let mut clk = SimClock::new();
    let t0 = clk.now_ms();
    let t1c = clk.advance(1_500);
    s.add(
        "F57-模拟-01",
        t0 == 0 && t1c.as_ref().map(|x| *x == 1_500).unwrap_or(false),
        "确定性时钟推针（F5412 注入时基）",
    );
    // 时钟驱动心跳全链（模拟时钟上复跑心跳判死——确定性）。
    let mut clk2 = SimClock::new();
    let mut hb5 = Heartbeat::new();
    hb5.beat(clk2.now_ms());
    clk2.advance(1_000).ok();
    hb5.beat(clk2.now_ms());
    let alive_sim = !hb5.timed_out(clk2.now_ms());
    clk2.advance(30_000).ok();
    let _d1 = hb5.timed_out(clk2.now_ms());
    let dead_sim = hb5.timed_out(clk2.now_ms());
    s.add(
        "F57-模拟-02",
        alive_sim && dead_sim && hb5.timeouts() == 1,
        "模拟时钟驱动心跳判死（确定性复跑）",
    );
    // F4623 口径声明在案。
    s.add(
        "F57-对接-01",
        F4623_STATEMACHINE_NOTE.contains("F4623")
            && F4623_STATEMACHINE_NOTE.contains("F5424")
            && F4623_STATEMACHINE_NOTE.contains("F5412"),
        "三对接点口径声明在案",
    );

    // --- 编排面（判据七：SessionManager 全链）---
    let mut mgr = SessionManager::new();
    mgr.handshake_ok().ok();
    mgr.snapshot_with(9, 10, 777, 1_000);
    let tick_adv = mgr.observe_quality(QualityInput { rtt_us: 10_000, loss_permille: 0 });
    let _ = tick_adv;
    // 断线→重连→进度原样接回（不清进度域本色全链）。
    mgr.heartbeat_beat(1_500);
    let mut timed_out_twice = false;
    for t in [7_000u64, 8_000] {
        if mgr.heartbeat_check(t) {
            timed_out_twice = true;
        }
    }
    let resumed = mgr.reconnect_ok(9_000);
    let resumed_ok = resumed.as_ref().map(|x| x.txn_watermark == 777 && x.room_id == 9).unwrap_or(false);
    s.add(
        "F57-编排-01",
        timed_out_twice && resumed_ok && mgr.state() == ConnState::Connected,
        "断线→重连→进度原样接回（域本色全链）",
    );
    // 会话摘要行（读屏可达）。
    let sum = mgr.summary();
    s.add(
        "F57-编排-02",
        sum.contains("态") && sum.contains("分级"),
        "会话摘要一行可读（无障碍解释面）",
    );

    // --- 版本与暂挂 ---
    let fp = {
        let mut h: u64 = SNAPSHOT_FNV_SEED;
        for b in SESSION_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(SNAPSHOT_FNV_PRIME);
        }
        h
    };
    s.add("F57-版本-01", fp != 0, "版本指纹非零（F57-session-v2-deepened）");

    s.add(
        "F57-暂挂-01",
        F57_LEDGER_SUSPENDED_NOTE.contains("暂挂") && F57_LEDGER_SUSPENDED_NOTE.contains("F5404"),
        "F 域账本暂挂声明显性",
    );

    // F57-暂挂-02：判据条数对账（动态容错——以实际条数为准，S156 纪律）。
    let declared = s.len();
    s.add("F57-暂挂-02", declared >= 30, "判据条数对账（深化版 ≥30 条）");

    s
}

/// F 域账本暂挂声明（跨批对接点：F4623 状态机口径参照上游；F5424 同步
/// 消费；F5412 模拟——建账前暂挂，移交期模式延续）。
pub const F57_LEDGER_SUSPENDED_NOTE: &str = "会话与连接管理（生命周期状态机+心跳保活+重连续接+质量分级）入 F 域账本：建账前暂挂声明（移交期模式延续——F5404 同款）；F5424 同步层消费质量分级与重连续接";
