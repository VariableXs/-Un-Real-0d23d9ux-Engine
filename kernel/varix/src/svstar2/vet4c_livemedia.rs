#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0xD700）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xD700;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CD7Code(pub u16);

impl CD7Code {
    pub const EMERGENCY_THROTTLED: Self = Self(SEG_BASE);   // 紧急被节流
    pub const RULER_HIDDEN: Self = Self(SEG_BASE + 1);      // 标尺隐藏
    pub const CAPTION_BREAK: Self = Self(SEG_BASE + 2);     // 实时字幕破门槛
    pub const COLLAB_DROP: Self = Self(SEG_BASE + 3);       // 协作断供
    pub const LATENCY_DRIFT: Self = Self(SEG_BASE + 4);     // 延迟漂移

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::EMERGENCY_THROTTLED => "紧急被节流",
            Self::RULER_HIDDEN => "标尺隐藏",
            Self::CAPTION_BREAK => "实时字幕破门槛",
            Self::COLLAB_DROP => "协作断供",
            Self::LATENCY_DRIFT => "延迟漂移",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 5] = [
        Self::EMERGENCY_THROTTLED, Self::RULER_HIDDEN, Self::CAPTION_BREAK,
        Self::COLLAB_DROP, Self::LATENCY_DRIFT,
    ];
}

// ── 实时延迟标尺（常显红线）────────────────────────────────────────

/// 实时延迟标尺：实时延迟必须实时常显。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveLatencyRuler {
    pub current_latency_ms: u32,
    pub visible: bool,
}

impl LiveLatencyRuler {
    /// 标尺常显红线：标尺必须可见。
    pub fn always_visible(&self) -> bool { self.visible }
    /// 延迟阈值告警：>3000ms 告警。
    pub fn over_threshold(&self) -> bool { self.current_latency_ms > 3000 }
}

// ── 紧急信息通道（保真红线）────────────────────────────────────────

/// 紧急信息优先级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrgencyLevel { Normal, High, Critical }

/// 紧急信息消息。
#[derive(Debug, Clone)]
pub struct EmergencyMsg {
    pub id: u32,
    pub level: UrgencyLevel,
    pub text: &'static str,
    /// 是否被节流（红线：紧急不得被节流丢弃）。
    pub throttled: bool,
    pub delivered: bool,
}

impl EmergencyMsg {
    /// 紧急保真：Critical 级不得被节流，且必须送达。
    pub fn fidelity_ok(&self) -> bool {
        if self.level == UrgencyLevel::Critical {
            !self.throttled && self.delivered
        } else {
            true
        }
    }
}

/// 紧急通道：消息队列 + 优先级抢占。
#[derive(Debug, Clone)]
pub struct EmergencyChannel {
    pub queue: Vec<EmergencyMsg>,
    pub override_active: bool,
}

impl EmergencyChannel {
    pub fn new() -> Self { Self { queue: Vec::new(), override_active: false } }

    /// 紧急注入：优先入队 + 触发覆盖渲染。
    pub fn inject(&mut self, msg: EmergencyMsg) {
        if msg.level == UrgencyLevel::Critical {
            self.queue.insert(0, msg);
            self.override_active = true;
        } else {
            self.queue.push(msg);
        }
    }

    /// 全部紧急消息保真。
    pub fn all_fidelity_ok(&self) -> bool {
        self.queue.iter().all(|m| m.fidelity_ok())
    }

    /// Critical 消息计数。
    pub fn critical_count(&self) -> usize {
        self.queue.iter().filter(|m| m.level == UrgencyLevel::Critical).count()
    }
}

// ── 协作实时无障碍（复用 F3845）────────────────────────────────────

/// 协作者在线状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollabPresence {
    pub user_id: u32,
    pub online: bool,
    pub announced: bool,
}

impl CollabPresence {
    /// 协作者状态播报：在线状态必须可播报。
    pub fn announce_ok(&self) -> bool { !self.online || self.announced }
}

/// 协作光标无障碍。
#[derive(Debug, Clone)]
pub struct CollabCursor {
    pub user_id: u32,
    pub x: u16, pub y: u16,
    pub rendered_label: bool,
}

impl CollabCursor {
    /// 协作光标无障碍：光标必须有可辨标签。
    pub fn a11y_ok(&self) -> bool { self.rendered_label }
}

// ── 实时描述预留（显性复述）────────────────────────────────────────

/// 实时场景描述接口预留。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveDescReserved {
    /// 预留是否显性声明（复述：预留必须显性）。
    pub explicitly_reserved: bool,
    pub implemented: bool,
}

impl LiveDescReserved {
    /// 预留显性：未实现时必须显性声明预留。
    pub fn reserve_ok(&self) -> bool { self.implemented || self.explicitly_reserved }
}

// ── 实时媒体会话 ────────────────────────────────────────────────────

/// 实时媒体无障碍会话。
#[derive(Debug, Clone)]
pub struct LiveMediaSession {
    pub ruler: LiveLatencyRuler,
    pub emergency: EmergencyChannel,
    pub collab: Vec<CollabPresence>,
    pub desc_reserved: LiveDescReserved,
}

impl LiveMediaSession {
    pub fn new() -> Self {
        Self {
            ruler: LiveLatencyRuler { current_latency_ms: 0, visible: true },
            emergency: EmergencyChannel::new(),
            collab: Vec::new(),
            desc_reserved: LiveDescReserved { explicitly_reserved: true, implemented: false },
        }
    }

    /// 综合审计。
    pub fn audit(&self) -> LiveAudit {
        LiveAudit {
            ruler_visible: self.ruler.always_visible(),
            emergency_ok: self.emergency.all_fidelity_ok(),
            collab_ok: self.collab.iter().all(|c| c.announce_ok()),
            desc_ok: self.desc_reserved.reserve_ok(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveAudit {
    pub ruler_visible: bool,
    pub emergency_ok: bool,
    pub collab_ok: bool,
    pub desc_ok: bool,
}
impl LiveAudit {
    pub fn all_green(&self) -> bool { self.ruler_visible && self.emergency_ok && self.collab_ok && self.desc_ok }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3865_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4c_livemedia");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 标尺常显
    {
        let r = LiveLatencyRuler { current_latency_ms: 100, visible: true };
        chk(&mut set, "C3865-01", r.always_visible(), "err");
    }
    // 2 标尺隐藏检出
    {
        let r = LiveLatencyRuler { current_latency_ms: 100, visible: false };
        chk(&mut set, "C3865-02", !r.always_visible(), "err");
    }
    // 3 延迟超阈值
    {
        let r = LiveLatencyRuler { current_latency_ms: 5000, visible: true };
        chk(&mut set, "C3865-03", r.over_threshold(), "err");
    }
    // 4 紧急保真（Critical+未节流+已送达）
    {
        let m = EmergencyMsg { id: 1, level: UrgencyLevel::Critical, text: "alert", throttled: false, delivered: true };
        chk(&mut set, "C3865-04", m.fidelity_ok(), "err");
    }
    // 5 紧急被节流=保真失败
    {
        let m = EmergencyMsg { id: 1, level: UrgencyLevel::Critical, text: "alert", throttled: true, delivered: false };
        chk(&mut set, "C3865-05", !m.fidelity_ok(), "err");
    }
    // 6 Normal 级不受保真限制
    {
        let m = EmergencyMsg { id: 1, level: UrgencyLevel::Normal, text: "info", throttled: true, delivered: true };
        chk(&mut set, "C3865-06", m.fidelity_ok(), "err");
    }
    // 7 紧急注入触发覆盖
    {
        let mut ch = EmergencyChannel::new();
        ch.inject(EmergencyMsg { id: 1, level: UrgencyLevel::Critical, text: "fire", throttled: false, delivered: true });
        chk(&mut set, "C3865-07", ch.override_active && ch.critical_count() == 1, "err");
    }
    // 8 紧急消息插队（Critical 在队首）
    {
        let mut ch = EmergencyChannel::new();
        ch.inject(EmergencyMsg { id: 1, level: UrgencyLevel::Normal, text: "n", throttled: false, delivered: true });
        ch.inject(EmergencyMsg { id: 2, level: UrgencyLevel::Critical, text: "c", throttled: false, delivered: true });
        chk(&mut set, "C3865-08", ch.queue[0].level == UrgencyLevel::Critical, "err");
    }
    // 9 协作者状态播报
    {
        let c = CollabPresence { user_id: 1, online: true, announced: true };
        chk(&mut set, "C3865-09", c.announce_ok(), "err");
    }
    // 10 协作者未播报检出
    {
        let c = CollabPresence { user_id: 1, online: true, announced: false };
        chk(&mut set, "C3865-10", !c.announce_ok(), "err");
    }
    // 11 协作光标无障碍
    {
        let c = CollabCursor { user_id: 1, x: 100, y: 200, rendered_label: true };
        chk(&mut set, "C3865-11", c.a11y_ok(), "err");
    }
    // 12 描述预留显性
    {
        let d = LiveDescReserved { explicitly_reserved: true, implemented: false };
        chk(&mut set, "C3865-12", d.reserve_ok(), "err");
    }
    // 13 描述预留不显性
    {
        let d = LiveDescReserved { explicitly_reserved: false, implemented: false };
        chk(&mut set, "C3865-13", !d.reserve_ok(), "err");
    }
    // 14 描述已实现
    {
        let d = LiveDescReserved { explicitly_reserved: false, implemented: true };
        chk(&mut set, "C3865-14", d.reserve_ok(), "err");
    }
    // 15 会话审计全绿
    {
        let s = LiveMediaSession::new();
        chk(&mut set, "C3865-15", s.audit().all_green(), "err");
    }
    // 16 会话审计标尺缺失
    {
        let mut s = LiveMediaSession::new();
        s.ruler.visible = false;
        chk(&mut set, "C3865-16", !s.audit().all_green(), "err");
    }
    // 17 高优先级非 Critical 也可保真
    {
        let m = EmergencyMsg { id: 1, level: UrgencyLevel::High, text: "warn", throttled: false, delivered: true };
        chk(&mut set, "C3865-17", m.fidelity_ok(), "err");
    }
    // 18 队列全保真
    {
        let mut ch = EmergencyChannel::new();
        ch.inject(EmergencyMsg { id: 1, level: UrgencyLevel::Critical, text: "a", throttled: false, delivered: true });
        ch.inject(EmergencyMsg { id: 2, level: UrgencyLevel::Normal, text: "b", throttled: false, delivered: true });
        chk(&mut set, "C3865-18", ch.all_fidelity_ok(), "err");
    }

    set
}
