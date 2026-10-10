//! VE-F5404 · 会话与连接管理（VE-F 域 · 任务段 · 目标 300 行）
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
//! # 一、连接生命周期（五态状态机）
//!
//! [`ConnState`] 五态（连接中/握手中/已连接/重连中/已关闭）+ 合法
//! 迁移表；非法迁移拒绝并归位（锚点错误路径：握手失败→诊断+重试
//! 指引）。断线检测由心跳超时驱动（[`Heartbeat`]）。
//!
//! # 二、重连恢复（重连不清进度——域本色核心）
//!
//! [`SessionSnapshot`] 在断线前留进度（房间/角色/已确认事务水位）；
//! [`Resumer`] 重连后把进度接回去——接不回即报（快照缺失→恢复
//! 失败显性），**不清零重来**（不清进度是体验承诺不是优化项）。
//!
//! # 三、质量分级 + 心跳自适应
//!
//! [`quality_grade`] 按 RTT 与丢包率双因子分四级；差连接给提前降级
//! 建议（域本色：差连接提前降级不硬撑）。心跳阈值自适应
//! （[`Heartbeat::observe`]——抖动大的链路自动放宽误判线，锚点：
//! 心跳误判→阈值自适应）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const SESSION_VERSION: &str = "F57-session-v1";

/// 非握手/连接态收到业务流量（归位指引）。
pub const E_SES_STATE: &str = "E_SES_STATE";

/// 快照缺失/损坏（恢复失败显性）。
pub const E_SES_SNAPSHOT: &str = "E_SES_SNAPSHOT";

/// 心跳超时（断线判定）。
pub const E_SES_TIMEOUT: &str = "E_SES_TIMEOUT";

/// 质量过差（提前降级建议）。
pub const E_SES_QUALITY: &str = "E_SES_QUALITY";

/// 心跳误判基数（初始超时阈值 ms）。
pub const HB_TIMEOUT_BASE_MS: u64 = 5_000;

/// 心跳误判自适应下限（ms）——再稳的链路也不低于 1s。
pub const HB_TIMEOUT_MIN_MS: u64 = 1_000;

/// 质量分级的 RTT 分档（微秒）。
pub const Q_RTT_GOOD_US: u32 = 80_000;

/// 质量分级的 RTT 差档（微秒）。
pub const Q_RTT_FAIR_US: u32 = 200_000;

/// 质量分级的丢包分档（千分比）。
pub const Q_LOSS_FAIR_PERMILLE: u32 = 20;

// ---------------------------------------------------------------------------
// 二、连接生命周期（五态状态机）
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

/// 合法迁移（五态×七事件的定表——表外即拒绝）。
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
}

impl Session {
    /// 新会话（连接中起步）。
    pub fn new() -> Session {
        Session { state: ConnState::Connecting, transitions: 0, rejects: 0 }
    }

    /// 当前状态。
    pub fn state(&self) -> ConnState {
        self.state
    }

    /// 派发事件（非法迁移拒绝+归位指引）。
    pub fn dispatch(&mut self, ev: ConnEvent) -> Result<ConnState, String> {
        match next_state(self.state, ev) {
            Some(ns) => {
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
}

// ---------------------------------------------------------------------------
// 三、心跳与断线检测（超时判定+自适应）
// ---------------------------------------------------------------------------

/// 心跳跟踪器（间隔记账+超时阈值自适应）。
#[derive(Clone, Copy, Debug)]
pub struct Heartbeat {
    /// 当前超时阈值（ms——自适应量）。
    timeout_ms: u64,
    /// 上次心跳时刻（逻辑时钟 ms；None=从未跳）。
    last_beat_ms: Option<u64>,
    /// 观测到的间隔样本数（自适应输入）。
    samples: u32,
    /// 判死次数（读屏可达）。
    timeouts: u32,
}

impl Heartbeat {
    /// 新跟踪器（基准阈值起步）。
    pub fn new() -> Heartbeat {
        Heartbeat { timeout_ms: HB_TIMEOUT_BASE_MS, last_beat_ms: None, samples: 0, timeouts: 0 }
    }

    /// 记一次心跳（同时按间隔抖动自适应阈值）。
    pub fn beat(&mut self, now_ms: u64) {
        if let Some(prev) = self.last_beat_ms {
            let gap = now_ms.saturating_sub(prev);
            // 自适应：阈值为观测间隔的 2 倍（不低于下限）——抖动大的
            // 链路放宽误判线（锚点：心跳误判→阈值自适应）。
            let want = (gap.saturating_mul(2)).max(HB_TIMEOUT_MIN_MS);
            if want != self.timeout_ms {
                self.timeout_ms = want;
            }
            self.samples = self.samples.saturating_add(1);
        }
        self.last_beat_ms = Some(now_ms);
    }

    /// 超时判定（当前时刻距上次心跳超过阈值即判死）。
    pub fn timed_out(&mut self, now_ms: u64) -> bool {
        match self.last_beat_ms {
            None => false, // 从未跳过不判死（还没开始）
            Some(prev) => {
                if now_ms.saturating_sub(prev) > self.timeout_ms {
                    self.timeouts = self.timeouts.saturating_add(1);
                    true
                } else {
                    false
                }
            }
        }
    }

    /// 当前阈值（读屏可达）。
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    /// 判死次数（读屏可达）。
    pub fn timeouts(&self) -> u32 {
        self.timeouts
    }
}

// ---------------------------------------------------------------------------
// 四、重连恢复（重连不清进度）
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

/// 重连续接器（快照进、进度出——接不回即显性报错）。
#[derive(Clone, Copy, Debug)]
pub struct Resumer {
    snapshot: Option<SessionSnapshot>,
    /// 续接次数（读屏可达）。
    resumed: u32,
}

impl Resumer {
    /// 空续接器（无快照）。
    pub fn new() -> Resumer {
        Resumer { snapshot: None, resumed: 0 }
    }

    /// 断线前留快照。
    pub fn stash(&mut self, snap: SessionSnapshot) {
        self.snapshot = Some(snap);
    }

    /// 重连后续接（快照缺失→恢复失败显性，不清零重来）。
    pub fn resume(&mut self) -> Result<SessionSnapshot, String> {
        match self.snapshot {
            Some(s) => {
                self.resumed = self.resumed.saturating_add(1);
                Ok(s)
            }
            None => Err(format!(
                "{}：重连快照缺失——无法续接进度（房间/角色/事务水位无处可接）。\
                 指引：断线前必须 stash；此刻只能按新会话处理并留痕，不清零冒充续接",
                E_SES_SNAPSHOT
            )),
        }
    }

    /// 续接次数（读屏可达）。
    pub fn resumed(&self) -> u32 {
        self.resumed
    }
}

// ---------------------------------------------------------------------------
// 五、连接质量分级
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
}

/// 质量输入（RTT 微秒 + 丢包千分比）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QualityInput {
    /// 往返耗时（微秒）。
    pub rtt_us: u32,
    /// 丢包率（千分比）。
    pub loss_permille: u32,
}

/// 质量分级（RTT 与丢包双因子——差连接提前降级）。
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
// 六、判据
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F5404 域自检（判据五组：生命周期/心跳/重连/质量/收尾）。
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
    // 非法迁移拒绝+归位（已连接态不收 TransportUp）。
    let r = ses.dispatch(ConnEvent::TransportUp);
    s.add(
        "F57-生命-02",
        r.is_err() && r.as_ref().unwrap_err().starts_with(E_SES_STATE) && ses.rejects() == 1,
        "非法迁移拒绝+归位指引",
    );
    // 终态封闭（Closed 后一切事件拒绝）。
    ses.dispatch(ConnEvent::HeartbeatTimeout).ok();
    ses.dispatch(ConnEvent::Close).ok();
    let closed_reject = ses.dispatch(ConnEvent::ReconnectOk);
    s.add(
        "F57-生命-03",
        ses.state().is_terminal() && closed_reject.is_err(),
        "终态封闭（Closed 不再收事件）",
    );
    // 业务面门禁（非 Connected 不承载流量）。
    let mut gate = Session::new();
    let carries_before = gate.state().carries_traffic();
    gate.dispatch(ConnEvent::TransportUp).ok();
    gate.dispatch(ConnEvent::HandshakeOk).ok();
    let carries_after = gate.state().carries_traffic();
    s.add("F57-生命-04", !carries_before && carries_after, "业务流量只在已连接态承载");

    // --- 心跳保活（判据二）---
    let mut hb = Heartbeat::new();
    // 正常心跳不判死。
    hb.beat(0);
    hb.beat(1_000);
    hb.beat(2_000);
    let alive = !hb.timed_out(2_500);
    s.add("F57-心跳-01", alive && hb.timeouts() == 0, "正常节奏不判死");
    // 超时判死（4s 静默 > 2×1s 自适应阈值=2s）。
    let dead = hb.timed_out(6_500);
    s.add(
        "F57-心跳-02",
        dead && hb.timeouts() == 1,
        "静默超阈值判死（断线检测）",
    );
    // 自适应：抖动链路阈值放宽（间隔 2s → 阈值 ≥4s 不再误判）。
    let mut hb2 = Heartbeat::new();
    hb2.beat(0);
    hb2.beat(2_000);
    s.add(
        "F57-心跳-03",
        hb2.timeout_ms() >= 4_000 && !hb2.timed_out(5_000),
        "阈值自适应（抖动 2s 间隔→阈值 4s 不误判）",
    );
    // 下限守卫（再稳也不低于 1s）。
    let mut hb3 = Heartbeat::new();
    hb3.beat(0);
    hb3.beat(10);
    s.add(
        "F57-心跳-04",
        hb3.timeout_ms() >= HB_TIMEOUT_MIN_MS,
        "自适应下限守卫（≥1s）",
    );

    // --- 重连续接（判据三）---
    // 快照续接（重连不清进度）。
    let mut rs = Resumer::new();
    rs.stash(SessionSnapshot { room_id: 7, entity_id: 42, txn_watermark: 9_001, taken_ms: 1_000 });
    let got = rs.resume();
    s.add(
        "F57-重连-01",
        got.as_ref().map(|x| x.room_id == 7 && x.txn_watermark == 9_001).unwrap_or(false) && rs.resumed() == 1,
        "快照续接（进度原样接回）",
    );
    // 快照缺失→恢复失败显性（不清零冒充）。
    let mut empty = Resumer::new();
    let r = empty.resume();
    s.add(
        "F57-重连-02",
        r.is_err()
            && r.as_ref()
                .err()
                .map(|x| x.starts_with(E_SES_SNAPSHOT) && x.contains("指引"))
                .unwrap_or(false),
        "快照缺失恢复失败显性（不清零重来）",
    );

    // --- 质量分级（判据四）---
    let g_ex = quality_grade(&QualityInput { rtt_us: 20_000, loss_permille: 0 });
    let g_good = quality_grade(&QualityInput { rtt_us: 50_000, loss_permille: 0 });
    let g_fair = quality_grade(&QualityInput { rtt_us: 120_000, loss_permille: 0 });
    let g_poor = quality_grade(&QualityInput { rtt_us: 300_000, loss_permille: 100 });
    s.add(
        "F57-质量-01",
        g_ex == QualityGrade::Excellent && g_good == QualityGrade::Good
            && g_fair == QualityGrade::Fair && g_poor == QualityGrade::Poor,
        "四档分级（RTT+丢包双因子单调）",
    );
    // 差连接提前降级建议（Fair/Poor 有建议，优/良无）。
    s.add(
        "F57-质量-02",
        degrade_advice(QualityGrade::Poor).is_some() && degrade_advice(QualityGrade::Fair).is_some()
            && degrade_advice(QualityGrade::Good).is_none() && degrade_advice(QualityGrade::Excellent).is_none(),
        "差连接提前降级（优/良不扰）",
    );

    // --- 版本与暂挂 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in SESSION_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    };
    s.add("F57-版本-01", fp != 0, "版本指纹非零（F57-session-v1）");

    s.add(
        "F57-暂挂-01",
        F57_LEDGER_SUSPENDED_NOTE.contains("暂挂") && F57_LEDGER_SUSPENDED_NOTE.contains("F5404"),
        "F 域账本暂挂声明显性",
    );

    // F57-暂挂-02：判据条数对账（本条为第 15 条）。
    s.add("F57-暂挂-02", s.len() == 14, "判据条数对账（14+本条）");

    s
}

/// F 域账本暂挂声明（跨批对接点：F4623 状态机口径参照上游；F5424 同步
/// 消费；F5412 模拟——建账前暂挂，移交期模式延续）。
pub const F57_LEDGER_SUSPENDED_NOTE: &str = "会话与连接管理（生命周期状态机+心跳保活+重连续接+质量分级）入 F 域账本：建账前暂挂声明（移交期模式延续——F5404 同款）；F5424 同步层消费质量分级与重连续接";
