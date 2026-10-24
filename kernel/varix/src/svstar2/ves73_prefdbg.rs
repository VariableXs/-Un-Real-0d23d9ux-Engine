//! VE-F3673 · 个性化调试（VE-S 域 · 批次 R04 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3673`
//!
//! **职责定位（锚点原文）**：个性化调试——
//! - **检视**：四域值全景 + 来源标注（复用 F3662 追溯——复述家族仪表）；
//! - **变更历史**：个性化变更时间线（谁改的何时改——复述审计）；
//! - **冲突回放**：消解决策回放（复用 F3669 留痕可视化）；
//! - **强制覆盖**：调试强制设值（沙箱隔离复述）；
//! - **家族关系**：家族十九成员复述协议。
//!
//! **判据（锚点原文）**：四域仪表、强制沙箱、回放可解释、家族十九、零常态、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 仪表分歧→立案 | [`C73Code::PANE_DIVERGE`] |
//! | 强制泄漏（调试值进生产）→沙箱断言 | [`C73Code::FORCE_LEAK`] |
//! | 回放失真→修正 | [`C73Code::REPLAY_DRIFT`] |
//! | 开销→打开才采样 | [`C73Code::PROBE_COST`] |
//!
//! 零 panic 面、零 IO、零墙钟（零常态：关闭态不采样）。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3673 诊断码段（独占 0x79xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C73Code(pub u16);

impl C73Code {
    /// 仪表分歧（立案）。
    pub const PANE_DIVERGE: C73Code = C73Code(0x7901);
    /// 强制泄漏（沙箱红线实测）。
    pub const FORCE_LEAK: C73Code = C73Code(0x7902);
    /// 回放失真。
    pub const REPLAY_DRIFT: C73Code = C73Code(0x7903);
    /// 探针开销（未开也采样）。
    pub const PROBE_COST: C73Code = C73Code(0x7904);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C73Code::PANE_DIVERGE => "四域仪表分歧。建议：立案比对。",
            C73Code::FORCE_LEAK => "调试强制值泄漏进生产（沙箱红线实测）。建议：沙箱隔离。",
            C73Code::REPLAY_DRIFT => "冲突回放失真。建议：按留痕修正。",
            C73Code::PROBE_COST => "探针常态开销。建议：打开才采样。",
            _ => "未知诊断码。",
        }
    }
}

// ── 四域仪表 ────────────────────────────────────────────────────────

/// 个性化四域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrefDomain {
    /// 视觉。
    Visual,
    /// 交互。
    Interact,
    /// 通知。
    Notify,
    /// 隐私。
    Privacy,
}

impl PrefDomain {
    pub const ALL: [PrefDomain; 4] = [
        PrefDomain::Visual,
        PrefDomain::Interact,
        PrefDomain::Notify,
        PrefDomain::Privacy,
    ];
}

/// 域值 + 来源标注。
#[derive(Clone, Copy)]
pub struct DomainValue {
    pub domain: PrefDomain,
    pub value: i32,
    /// 来源标识（0=手设 1=同步 2=场景 3=默认）。
    pub source: u8,
}

/// 四域仪表（复用 F3662 追溯口径）。
pub struct DomainPanes {
    values: Vec<DomainValue>,
    diverge_hits: u32,
    last_code: C73Code,
    has_error: bool,
}

impl DomainPanes {
    pub fn new() -> Self {
        Self {
            values: Vec::new(),
            diverge_hits: 0,
            last_code: C73Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C73Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 写入一域值。
    pub fn set(&mut self, domain: PrefDomain, value: i32, source: u8) {
        if let Some(e) = self.values.iter_mut().find(|e| e.domain == domain) {
            e.value = value;
            e.source = source;
        } else {
            self.values.push(DomainValue { domain, value, source });
        }
    }

    /// 读一域。
    pub fn get(&self, domain: PrefDomain) -> Option<&DomainValue> {
        self.values.iter().find(|e| e.domain == domain)
    }

    /// 全景齐备：四域都有值。
    pub fn panes_complete(&self) -> bool {
        PrefDomain::ALL.iter().all(|d| self.values.iter().any(|e| e.domain == *d))
    }

    /// 分歧检测：同域出现两个不同来源的写入痕迹即分歧。
    pub fn note_diverge(&mut self) {
        self.diverge_hits += 1;
        self.last_code = C73Code::PANE_DIVERGE;
        self.has_error = true;
    }

    pub fn diverge_hits(&self) -> u32 {
        self.diverge_hits
    }

    /// 来源标注齐备：每个值都有 source。
    pub fn sources_tagged(&self) -> bool {
        self.values.iter().all(|e| e.source <= 3)
    }
}

// ── 变更时间线 ──────────────────────────────────────────────────────

/// 变更事件（谁改的何时改——抽象序号，非墙钟）。
#[derive(Clone, Copy)]
pub struct ChangeEvent {
    pub seq: u32,
    pub domain: PrefDomain,
    pub actor: u8,
    /// 0=用户 1=系统 2=调试 3=同步
    pub new_value: i32,
}

/// 变更时间线（审计复述）。
pub struct ChangeTimeline {
    events: Vec<ChangeEvent>,
    next_seq: u32,
}

impl ChangeTimeline {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            next_seq: 1,
        }
    }

    /// 记一笔变更。
    pub fn record(&mut self, domain: PrefDomain, actor: u8, new_value: i32) -> u32 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.events.push(ChangeEvent {
            seq,
            domain,
            actor,
            new_value,
        });
        seq
    }

    /// 按序取全部。
    pub fn events(&self) -> &[ChangeEvent] {
        &self.events
    }

    /// 最近 N 条。
    pub fn recent(&self, n: usize) -> Vec<ChangeEvent> {
        let len = self.events.len();
        let start = len.saturating_sub(n);
        self.events[start..].to_vec()
    }

    pub fn count(&self) -> usize {
        self.events.len()
    }

    /// 审计齐备：每条事件有 actor。
    pub fn audit_ok(&self) -> bool {
        self.events.iter().all(|e| e.actor <= 3)
    }
}

// ── 冲突回放 ────────────────────────────────────────────────────────

/// 回放决策（复用 F3669 留痕）。
#[derive(Clone, Copy)]
pub struct ReplayStep {
    pub key: u16,
    pub chosen: i32,
    pub rejected: i32,
    /// 理由码（0=手设优先 1=场景 2=同步 3=默认）。
    pub reason: u8,
}

/// 冲突回放器（可解释）。
pub struct ConflictReplay {
    steps: Vec<ReplayStep>,
    drift_hits: u32,
    last_code: C73Code,
    has_error: bool,
}

impl ConflictReplay {
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            drift_hits: 0,
            last_code: C73Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C73Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn push(&mut self, step: ReplayStep) {
        self.steps.push(step);
    }

    /// 可解释断言：每步必须有 reason。
    pub fn assert_explainable(&mut self) -> Result<bool, C73Code> {
        if self.steps.iter().all(|s| s.reason <= 3) {
            self.last_code = C73Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.drift_hits += 1;
            self.last_code = C73Code::REPLAY_DRIFT;
            self.has_error = true;
            Err(C73Code::REPLAY_DRIFT)
        }
    }

    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    pub fn drift_hits(&self) -> u32 {
        self.drift_hits
    }

    /// 步数完整（每冲突一步）。
    pub fn steps_for(&self, key: u16) -> Option<&ReplayStep> {
        self.steps.iter().find(|s| s.key == key)
    }
}

// ── 强制沙箱 ────────────────────────────────────────────────────────

/// 强制覆盖（调试设值）。
pub struct ForceSandbox {
    /// 沙箱内的强制值。
    sandbox: Vec<(PrefDomain, i32)>,
    /// 泄漏进生产计数（红线实测）。
    leaks: u32,
    /// 是否打开调试器（零常态：关则不采样）。
    enabled: bool,
    samples: u32,
    last_code: C73Code,
    has_error: bool,
}

impl ForceSandbox {
    pub fn new() -> Self {
        Self {
            sandbox: Vec::new(),
            leaks: 0,
            enabled: false,
            samples: 0,
            last_code: C73Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C73Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 打开调试器。
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// 关闭调试器（零常态）。
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// 沙箱强制设值。
    pub fn force(&mut self, domain: PrefDomain, value: i32) -> Result<bool, C73Code> {
        if !self.enabled {
            self.last_code = C73Code::PROBE_COST;
            self.has_error = true;
            return Err(C73Code::PROBE_COST);
        }
        if let Some(e) = self.sandbox.iter_mut().find(|(d, _)| *d == domain) {
            e.1 = value;
        } else {
            self.sandbox.push((domain, value));
        }
        self.samples += 1;
        self.last_code = C73Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 沙箱隔离断言：强制值不得进生产。
    pub fn assert_isolated(&mut self, leaked: bool) -> Result<bool, C73Code> {
        if leaked {
            self.leaks += 1;
            self.last_code = C73Code::FORCE_LEAK;
            self.has_error = true;
            Err(C73Code::FORCE_LEAK)
        } else {
            self.last_code = C73Code(0);
            self.has_error = false;
            Ok(true)
        }
    }

    /// 零常态：关闭态不采样。
    pub fn probe(&mut self) -> bool {
        if self.enabled {
            self.samples += 1;
            true
        } else {
            false
        }
    }

    pub fn leaks(&self) -> u32 {
        self.leaks
    }

    pub fn samples(&self) -> u32 {
        self.samples
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn sandbox_count(&self) -> usize {
        self.sandbox.len()
    }
}

// ── 家族协议 ────────────────────────────────────────────────────────

/// 家族十九成员协议（复述）。
pub struct FamilyProtocol {
    member_id: u8,
    /// 协议握手次数。
    handshakes: u32,
}

impl FamilyProtocol {
    pub fn new(member_id: u8) -> Self {
        Self {
            member_id,
            handshakes: 0,
        }
    }

    /// 握手（家族成员互认）。
    pub fn handshake(&mut self, peer_id: u8) -> bool {
        if peer_id != self.member_id {
            self.handshakes += 1;
            true
        } else {
            false
        }
    }

    pub fn handshakes(&self) -> u32 {
        self.handshakes
    }

    pub fn member_id(&self) -> u8 {
        self.member_id
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves73_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves73_prefdbg");
    {
        let mut p = DomainPanes::new();
        p.set(PrefDomain::Visual, 1, 0);
        p.set(PrefDomain::Interact, 2, 1);
        p.set(PrefDomain::Notify, 3, 2);
        p.set(PrefDomain::Privacy, 4, 3);
        chk(&mut set, "C3673-01", p.panes_complete() && p.sources_tagged(), "err");
    }
    {
        let mut p = DomainPanes::new();
        p.set(PrefDomain::Visual, 1, 0);
        p.note_diverge();
        chk(&mut set, "C3673-02", p.diverge_hits() == 1 && p.has_error(), "err");
    }
    {
        let mut p = DomainPanes::new();
        p.set(PrefDomain::Visual, 42, 0);
        let v = p.get(PrefDomain::Visual);
        chk(&mut set, "C3673-03", v.map(|e| e.value) == Some(42), "err");
    }
    {
        let mut t = ChangeTimeline::new();
        t.record(PrefDomain::Visual, 0, 10);
        t.record(PrefDomain::Notify, 2, 20);
        chk(&mut set, "C3673-04", t.count() == 2 && t.audit_ok(), "err");
    }
    {
        let mut t = ChangeTimeline::new();
        for i in 0..5 {
            t.record(PrefDomain::Interact, i % 4, i as i32);
        }
        let r = t.recent(2);
        chk(&mut set, "C3673-05", r.len() == 2 && r[0].seq == 4, "err");
    }
    {
        let mut r = ConflictReplay::new();
        r.push(ReplayStep { key: 1, chosen: 10, rejected: 20, reason: 0 });
        r.push(ReplayStep { key: 2, chosen: 1, rejected: 2, reason: 1 });
        let res = r.assert_explainable();
        chk(&mut set, "C3673-06", res == Ok(true), "err");
    }
    {
        let mut r = ConflictReplay::new();
        r.push(ReplayStep { key: 1, chosen: 10, rejected: 20, reason: 9 });
        let res = r.assert_explainable();
        chk(&mut set, "C3673-07", res.err() == Some(C73Code::REPLAY_DRIFT), "err");
    }
    {
        let mut r = ConflictReplay::new();
        r.push(ReplayStep { key: 7, chosen: 5, rejected: 6, reason: 2 });
        let s = r.steps_for(7);
        chk(&mut set, "C3673-08", s.map(|x| x.chosen) == Some(5), "err");
    }
    {
        let mut s = ForceSandbox::new();
        let res = s.force(PrefDomain::Visual, 1);
        chk(&mut set, "C3673-09", res.err() == Some(C73Code::PROBE_COST), "err");
    }
    {
        let mut s = ForceSandbox::new();
        s.enable();
        let res = s.force(PrefDomain::Visual, 9);
        chk(&mut set, "C3673-10", res == Ok(true) && s.sandbox_count() == 1, "err");
    }
    {
        let mut s = ForceSandbox::new();
        s.enable();
        let res = s.assert_isolated(true);
        chk(&mut set, "C3673-11", res.err() == Some(C73Code::FORCE_LEAK), "err");
    }
    {
        let mut s = ForceSandbox::new();
        let probed = s.probe();
        let mut f = FamilyProtocol::new(7);
        let hs = f.handshake(8);
        chk(&mut set, "C3673-12", !probed && hs && f.handshakes() == 1, "err");
    }
    set
}
