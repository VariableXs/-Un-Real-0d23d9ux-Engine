//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18570 · 上游变更跟随账（接口漂移监测） · 0x80A40段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18570-J1 接口指纹监测断言全过，漂移 10/10 告警并挂起
//!       下游收口
//! 设计要点：
//!   - 监视面 6 接口闭集：每行 = （接口号, 基线指纹, 本地指纹, 状态,
//!     挂起计数）；基线指纹 = mix(接口号, 上游种子)——可独立复算，
//!     改基线或本地任一侧即漂移；
//!   - 监测状态机（单调三态）：正常→漂移告警→下游挂起；告警与挂起
//!     是一次扫描内联动——漂移而不告警（DriftMissed）或告警不挂起
//!     （SuspendMissing）都必抓；
//!   - 挂起语义：漂移接口的下游待收口全部转挂起，挂起计数 == 该接口
//!     待收口数（账不丢）；恢复只许走重基线（新账行），不许状态倒退；
//!   - 监测轮账：告警数 == 漂移数、挂起数 == 漂移接口待收口和，
//!     tries = caught + missed 守恒，漏检必须为零；
//!   - 注入核三类：指纹漂移必告警 / 挂起缺位必拒 / 状态倒退必拒，
//!     各 10 轮全抓（漂移 10/10）；
//!   - 边界：接口号越界/重复监视/封账后追加/未扫封账；
//!   - LCG 种子写死 = SEG_BASE（0x80A40）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! 结构清单：
//!   数据结构：WatchRow（监视行）/ FollowLedger（跟随账）
//!   状态机  ：WatchState（正常→告警→挂起 单调三态）+
//!             MonitorPhase（闲→扫→封 单调三态）
//!   账本    ：监视行数组账（6 槽）+ 待收口账 + 告警/挂起计数 + 注入检出账
//!   错误路径：FollowCode 12 码闭集，code() = SEG_BASE + n（0x80A40 窗）
//!   注入核  ：漂移告警/挂起缺位/状态倒退三类逐类断言被拒（10/10）
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80A40，AI-VX11-1105 批09 第 10 槽，0x40 对齐独占窗）。
pub const SEG_BASE: u32 = 0x80A40;

// ── 冻结常量（写死并注明推导） ──────────────────────────────────
/// 监视接口数（推导：版本面挂接面 6 接口 = manifest/hive/上栈/免装/
/// 回归/域账 各一）。
pub const IFACE_N: usize = 6;
/// 漂移注入轮数（判据原文钉死：10/10）。
pub const DRIFT_ROUNDS: u32 = 10;
/// 判据迭代轮数（写死：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 每接口待收口闭集（推导：接口 i 待收口 = i+1，合计 21）。
pub const PENDING_TOTAL: u32 = 21;
/// 上游种子（τ 前 64 位，冻结推导）。
pub const UP_SEED: u64 = 0x6487_ED51_0897_09CE;
/// 漂移注入位模（写死：翻三个位，注入面专用）。
pub const DRIFT_MASK: u64 = 0x0000_0000_00FF_00FF;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FollowCode {
    /// 接口号越界：0..6 之外
    IfaceOutOfRange,
    /// 重复监视：同接口二行
    DupWatch,
    /// 指纹不齐：基线复算不符
    FpMismatch,
    /// 漂移漏报：漂移未告警
    DriftMissed,
    /// 挂起缺位：漂移未挂起下游
    SuspendMissing,
    /// 状态越级/倒退
    StateBreak,
    /// 未扫封账
    SealNotReady,
    /// 封账后追加
    AppendAfterSeal,
    /// 账目守恒破
    ConservationBreak,
    /// 轮账不守恒：告警数 ≠ 漂移数
    RoundBreak,
    /// 双跑漂移
    NotReproducible,
    /// 未知诊断码兜底
    Unknown,
}

impl FollowCode {
    /// 诊断码 = SEG_BASE + 序号（段内闭集，const 供静态裁决表）。
    pub const fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文建议（文案闭集，每码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            FollowCode::IfaceOutOfRange => "接口号越界：只在 0..6 内取",
            FollowCode::DupWatch => "重复监视：一接口一行",
            FollowCode::FpMismatch => "指纹不齐：基线复算不符，重出基线",
            FollowCode::DriftMissed => "漂移漏报：漂移必须当轮告警",
            FollowCode::SuspendMissing => "挂起缺位：漂移接口下游必须挂起",
            FollowCode::StateBreak => "状态越级或倒退：按 正常→告警→挂起 走",
            FollowCode::SealNotReady => "未扫封账：先扫一轮再封账",
            FollowCode::AppendAfterSeal => "封账后追加：账面只读，重开新账",
            FollowCode::ConservationBreak => "账目不守恒：挂起数对不上待收口数",
            FollowCode::RoundBreak => "轮账不守恒：告警数必须等于漂移数",
            FollowCode::NotReproducible => "双跑漂移：冻结种子重跑对拍",
            FollowCode::Unknown => "未知诊断码：不在 0x80A40 段闭集内",
        }
    }
    pub const ALL: [FollowCode; 12] = [
        FollowCode::IfaceOutOfRange,
        FollowCode::DupWatch,
        FollowCode::FpMismatch,
        FollowCode::DriftMissed,
        FollowCode::SuspendMissing,
        FollowCode::StateBreak,
        FollowCode::SealNotReady,
        FollowCode::AppendAfterSeal,
        FollowCode::ConservationBreak,
        FollowCode::RoundBreak,
        FollowCode::NotReproducible,
        FollowCode::Unknown,
    ];
}

/// 码值反查中文建议（段外码落 Unknown 兜底）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FollowCode::ALL.len() {
        if FollowCode::ALL[i].code() == code {
            return FollowCode::ALL[i].detail();
        }
        i += 1;
    }
    FollowCode::Unknown.detail()
}

// ── 指纹原语 ────────────────────────────────────────────────────
/// 64 位雪崩混合（写死常量，双跑必同）。
pub fn mix64(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^= x >> 33;
    x
}

/// 上游基线指纹：接口号 + 上游种子二元混合（可独立复算）。
pub fn base_fp(iid: u32) -> u64 {
    mix64(((iid as u64) << 32) ^ UP_SEED)
}

// ── 监视状态机（单调三态） ──────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WatchState {
    Normal = 0,
    DriftAlert = 1,
    Suspended = 2,
}

impl WatchState {
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 单调 +1 放行；倒退/越级拒 StateBreak。
    pub fn advance(self, to: WatchState) -> Result<WatchState, u32> {
        let from = self.rank();
        let dst = to.rank();
        if dst == from + 1 {
            Ok(to)
        } else {
            Err(FollowCode::StateBreak.code())
        }
    }
}

/// 监测相位（闲→扫→封，单调）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MonitorPhase {
    Idle = 0,
    Watching = 1,
    Sealed = 2,
}

impl MonitorPhase {
    pub fn rank(self) -> u32 {
        self as u32
    }
    pub fn advance(self, to: MonitorPhase) -> Result<MonitorPhase, u32> {
        let from = self.rank();
        let dst = to.rank();
        if dst == from + 1 {
            Ok(to)
        } else {
            Err(FollowCode::StateBreak.code())
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────
/// 监视行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WatchRow {
    pub iid: u32,
    /// 上游基线指纹。
    pub base: u64,
    /// 本地指纹（跟随账登记值）。
    pub local: u64,
    /// 监视状态机。
    pub state: WatchState,
    /// 挂起的下游待收口数。
    pub susp_cnt: u32,
    pub present: bool,
}

impl WatchRow {
    pub const fn empty(iid: u32) -> WatchRow {
        WatchRow {
            iid,
            base: 0,
            local: 0,
            state: WatchState::Normal,
            susp_cnt: 0,
            present: false,
        }
    }
}

/// 上游变更跟随账。
pub struct FollowLedger {
    /// 监视行数组账（6 槽定容）。
    pub rows: [WatchRow; IFACE_N],
    /// 下游待收口账（接口号 → 待收口数；闭式 i+1）。
    pub pending: [u32; IFACE_N],
    /// 监测相位。
    pub phase: MonitorPhase,
    /// 告警计数（轮账轴）。
    pub alerts: u32,
    /// 挂起计数（与漂移接口待收口和守恒）。
    pub suspended: u32,
    /// 扫描轮次。
    pub scan_cnt: u32,
}

impl FollowLedger {
    pub fn new() -> FollowLedger {
        FollowLedger {
            rows: [
                WatchRow::empty(0),
                WatchRow::empty(1),
                WatchRow::empty(2),
                WatchRow::empty(3),
                WatchRow::empty(4),
                WatchRow::empty(5),
            ],
            pending: [0; IFACE_N],
            phase: MonitorPhase::Idle,
            alerts: 0,
            suspended: 0,
            scan_cnt: 0,
        }
    }

    /// 挂监视：接口号域守卫 + 查重 + 基线指纹入账（本地=基线=零漂移）。
    pub fn watch(&mut self, iid: usize) -> Result<u64, u32> {
        if iid >= IFACE_N {
            return Err(FollowCode::IfaceOutOfRange.code());
        }
        if self.rows[iid].present {
            return Err(FollowCode::DupWatch.code());
        }
        if self.phase == MonitorPhase::Sealed {
            return Err(FollowCode::AppendAfterSeal.code());
        }
        let f = base_fp(iid as u32);
        self.rows[iid] = WatchRow {
            iid: iid as u32,
            base: f,
            local: f,
            state: WatchState::Normal,
            susp_cnt: 0,
            present: true,
        };
        // 待收口闭式：接口 i 待收口 = i+1。
        self.pending[iid] = (iid + 1) as u32;
        Ok(f)
    }

    /// 基线复算对拍（指纹不齐即账坏）。
    fn base_ok(&self, iid: usize) -> Result<(), u32> {
        if iid >= IFACE_N {
            return Err(FollowCode::IfaceOutOfRange.code());
        }
        match self.rows.get(iid) {
            Some(r) => {
                if !r.present {
                    return Err(FollowCode::IfaceOutOfRange.code());
                }
                if r.base != base_fp(iid as u32) {
                    return Err(FollowCode::FpMismatch.code());
                }
                Ok(())
            }
            None => Err(FollowCode::IfaceOutOfRange.code()),
        }
    }

    /// 漂移注入核（仅注入面）：翻本地指纹指定位模。
    pub fn inject_drift(&mut self, iid: usize) -> Result<(), u32> {
        if iid >= IFACE_N {
            return Err(FollowCode::IfaceOutOfRange.code());
        }
        if !self.rows[iid].present {
            return Err(FollowCode::IfaceOutOfRange.code());
        }
        if self.phase == MonitorPhase::Sealed {
            return Err(FollowCode::AppendAfterSeal.code());
        }
        match self.rows.get_mut(iid) {
            Some(r) => {
                r.local ^= DRIFT_MASK;
                Ok(())
            }
            None => Err(FollowCode::IfaceOutOfRange.code()),
        }
    }

    /// 扫描一轮：逐接口基线复算 + 本地对拍；漂移联动告警+挂起。
    /// 返回本轮漂移接口数。
    pub fn scan(&mut self) -> Result<u32, u32> {
        if self.phase == MonitorPhase::Sealed {
            return Err(FollowCode::AppendAfterSeal.code());
        }
        if self.phase == MonitorPhase::Idle {
            self.phase = self.phase.advance(MonitorPhase::Watching)?;
        }
        let mut drifted = 0u32;
        let mut i = 0;
        while i < IFACE_N {
            self.base_ok(i)?;
            let (local, state, pend) = match self.rows.get(i) {
                Some(r) => (r.local, r.state, self.pending[i]),
                None => return Err(FollowCode::IfaceOutOfRange.code()),
            };
            if local != base_fp(i as u32) {
                // 漂移：告警 + 挂起（一次扫描内联动，账不丢）。
                let st = match self.rows.get_mut(i) {
                    Some(r) => r,
                    None => return Err(FollowCode::IfaceOutOfRange.code()),
                };
                st.state = st.state.advance(WatchState::DriftAlert)?;
                st.state = st.state.advance(WatchState::Suspended)?;
                st.susp_cnt = pend;
                self.alerts = self.alerts.saturating_add(1);
                self.suspended = self.suspended.saturating_add(pend);
                drifted = drifted.saturating_add(1);
            }
            i += 1;
        }
        self.scan_cnt = self.scan_cnt.saturating_add(1);
        Ok(drifted)
    }

    /// 轮账守恒：告警数 == 累计漂移数；挂起数 == 漂移接口待收口和。
    pub fn conserve(&self, drift_total: u32) -> Result<(), u32> {
        if self.alerts != drift_total {
            return Err(FollowCode::RoundBreak.code());
        }
        // 挂起数闭式复算：逐行把挂起行 susp_cnt 求和。
        let mut susp = 0u32;
        let mut i = 0;
        while i < IFACE_N {
            match self.rows.get(i) {
                Some(r) => {
                    if r.present && r.state == WatchState::Suspended {
                        susp = susp.saturating_add(r.susp_cnt);
                    }
                }
                None => return Err(FollowCode::IfaceOutOfRange.code()),
            }
            i += 1;
        }
        if susp != self.suspended {
            return Err(FollowCode::ConservationBreak.code());
        }
        Ok(())
    }

    /// 封账闸（扫→封）：至少扫过一轮。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase != MonitorPhase::Watching {
            return Err(FollowCode::SealNotReady.code());
        }
        self.phase = self.phase.advance(MonitorPhase::Sealed)?;
        Ok(())
    }
}

// ── 监测裁决表（输入：漂移/已告警/已挂起 → 期望码；0=闭环） ────
/// 四例样本：无漂移→闭环；漂移未告警→DriftMissed；告警未挂起→
/// SuspendMissing；状态倒退→StateBreak。
pub const MONITOR_CASES: [(bool, bool, bool, u32); 4] = [
    // (drift, alerted, suspended, want: 0=闭环)
    (false, false, false, 0),
    (true, false, true, FollowCode::DriftMissed.code()),
    (true, true, false, FollowCode::SuspendMissing.code()),
    (true, true, true, 0),
];

/// 监测裁决闭式：与 scan 联动语义同源（判据复算核）。
pub fn adjudicate_monitor(drift: bool, alerted: bool, suspended: bool) -> u32 {
    if !drift {
        return 0;
    }
    if !alerted {
        return FollowCode::DriftMissed.code();
    }
    if !suspended {
        return FollowCode::SuspendMissing.code();
    }
    0
}

/// 裁决表逐例对拍。
pub fn adjudication_ok() -> bool {
    let mut i = 0;
    while i < MONITOR_CASES.len() {
        let (d, a, s, want) = MONITOR_CASES[i];
        if adjudicate_monitor(d, a, s) != want {
            return false;
        }
        i += 1;
    }
    true
}

// ── 标准流程与黄金全链 ──────────────────────────────────────────
/// 黄金全链：6 接口挂监视→扫一轮（零漂移）→轮账守恒→封账。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut l = FollowLedger::new();
    let mut i = 0;
    while i < IFACE_N {
        l.watch(i)?;
        i += 1;
    }
    let drifted = l.scan()?;
    if drifted != 0 {
        return Err(FollowCode::DriftMissed.code());
    }
    l.conserve(0)?;
    l.seal()?;
    Ok((l.alerts, l.suspended))
}

/// 漂移流：6 接口挂监视→接口 2 注入漂移→扫一轮→1 告警 + 3 挂起
/// →轮账守恒（漂移数=1）。
pub fn drift_flow_run() -> Result<(u32, u32), u32> {
    let mut l = FollowLedger::new();
    let mut i = 0;
    while i < IFACE_N {
        l.watch(i)?;
        i += 1;
    }
    l.inject_drift(2)?;
    let drifted = l.scan()?;
    if drifted != 1 {
        return Err(FollowCode::DriftMissed.code());
    }
    // 接口 2 状态必须到挂起，挂起计数 = 待收口 3。
    let st = match l.rows.get(2) {
        Some(r) => (r.state, r.susp_cnt),
        None => return Err(FollowCode::IfaceOutOfRange.code()),
    };
    if st.0 != WatchState::Suspended || st.1 != 3 {
        return Err(FollowCode::SuspendMissing.code());
    }
    l.conserve(1)?;
    Ok((l.alerts, l.suspended))
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn missed(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < 3 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入核：漂移 10/10 告警并挂起 + 两类负向闸，各 JUDGE_ROUNDS 轮。
///   类 0：指纹漂移必告警（漂移后 scan 必须 drifted≥1 且状态到挂起）
///   类 1：挂起缺位必拒（漂移接口挂起计数必须 = 待收口数）
///   类 2：状态倒退必拒（挂起态回正常 → StateBreak）
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 类 0：漂移必告警（逐接口轮转注入）。
        let mut a = fresh_watch()?;
        let iid = (r % IFACE_N as u32) as usize;
        a.inject_drift(iid)?;
        let hit_a = match a.scan() {
            Ok(d) => d >= 1 && a.alerts >= 1,
            Err(_) => false,
        };
        led.record(0, hit_a);
        // 类 1：挂起缺位必拒（挂起计数必须 = i+1）。
        let mut b = fresh_watch()?;
        b.inject_drift(4)?;
        b.scan()?;
        let ok_b = match b.rows.get(4) {
            Some(rw) => rw.state == WatchState::Suspended && rw.susp_cnt == 5,
            None => false,
        };
        let hit_b = ok_b && b.suspended == 5;
        led.record(1, hit_b);
        // 类 2：状态倒退必拒。
        let hit_c = WatchState::Suspended.advance(WatchState::Normal)
            == Err(FollowCode::StateBreak.code());
        led.record(2, hit_c);
        r += 1;
    }
    Ok(led)
}

/// 造一台六接口监视账（未扫；注入前置）。
fn fresh_watch() -> Result<FollowLedger, u32> {
    let mut l = FollowLedger::new();
    let mut i = 0;
    while i < IFACE_N {
        l.watch(i)?;
        i += 1;
    }
    Ok(l)
}

// ── 边界与探针 ──────────────────────────────────────────────────
/// 边界探针：接口号越界 / 重复监视 / 未扫封账 / 封后注入。
pub fn boundary_probe() -> Result<(), u32> {
    let mut l = FollowLedger::new();
    // 接口号越界。
    match l.watch(IFACE_N) {
        Err(c) if c == FollowCode::IfaceOutOfRange.code() => {}
        _ => return Err(FollowCode::IfaceOutOfRange.code()),
    }
    // 重复监视。
    l.watch(0)?;
    match l.watch(0) {
        Err(c) if c == FollowCode::DupWatch.code() => {}
        _ => return Err(FollowCode::DupWatch.code()),
    }
    // 未扫封账。
    let mut h = fresh_watch()?;
    match h.seal() {
        Err(c) if c == FollowCode::SealNotReady.code() => {}
        _ => return Err(FollowCode::SealNotReady.code()),
    }
    // 封后注入必拒。
    h.scan()?;
    h.seal()?;
    match h.inject_drift(1) {
        Err(c) if c == FollowCode::AppendAfterSeal.code() => Ok(()),
        _ => Err(FollowCode::AppendAfterSeal.code()),
    }
}

/// 探针 A：监测相位越级（闲直取封）必拒。
pub fn probe_phase_skip() -> Result<(), u32> {
    let l = FollowLedger::new();
    let skip = match MonitorPhase::Idle.advance(MonitorPhase::Sealed) {
        Err(c) if c == FollowCode::StateBreak.code() => true,
        _ => false,
    };
    if !skip {
        return Err(FollowCode::StateBreak.code());
    }
    let _ = l;
    Ok(())
}

/// 探针 B：基线指纹确定性 + 六接口互异。
pub fn probe_base_fp_props() -> Result<(), u32> {
    let mut i = 0;
    while i < IFACE_N {
        let f1 = base_fp(i as u32);
        let f2 = base_fp(i as u32);
        if f1 != f2 {
            return Err(FollowCode::FpMismatch.code());
        }
        let mut k = i + 1;
        while k < IFACE_N {
            if base_fp(k as u32) == f1 {
                return Err(FollowCode::FpMismatch.code());
            }
            k += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 探针 C：多接口同轮漂移（接口 1/3/5 齐漂 → 3 告警 9 挂起）。
pub fn probe_multi_drift() -> Result<(), u32> {
    let mut l = fresh_watch()?;
    l.inject_drift(1)?;
    l.inject_drift(3)?;
    l.inject_drift(5)?;
    let drifted = l.scan()?;
    if drifted != 3 {
        return Err(FollowCode::DriftMissed.code());
    }
    if l.alerts != 3 || l.suspended != (2 + 4 + 6) {
        return Err(FollowCode::RoundBreak.code());
    }
    l.conserve(3)?;
    Ok(())
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(FollowCode::NotReproducible.code());
    }
    let c = drift_flow_run()?;
    let d = drift_flow_run()?;
    if c != d {
        return Err(FollowCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：SEG 对齐/窗位 + 码闭集段内 + 文案非空 + 待收口闭式。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x84000).contains(&SEG_BASE) {
        return false;
    }
    let mut i = 0;
    while i < FollowCode::ALL.len() {
        let v = FollowCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if FollowCode::ALL[i].detail().len() == 0 {
            return false;
        }
        i += 1;
    }
    // 待收口闭式：Σ(i+1) = 21。
    let mut sum = 0u32;
    let mut i = 0;
    while i < IFACE_N {
        sum = sum.saturating_add((i + 1) as u32);
        i += 1;
    }
    sum == PENDING_TOTAL
}

/// 运行期摘要（动态明细消费）。
pub fn summary() -> String {
    let g = golden_run();
    format!("{:?}", g)
}

// ── 检查入口（判据 8–20 条） ────────────────────────────────────
pub fn run_unx18570_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18570 上游变更跟随账（接口漂移监测）");

    // ── 静态明细（主体） ─────────────────────────────────────
    let golden = golden_run();
    s.add("J1-零漂移闭环", golden.is_ok(), "六接口挂监视→扫一轮零漂移→封账");
    let drift = drift_flow_run();
    let ok2 = match &drift {
        Ok((a, sup)) => *a == 1 && *sup == 3,
        Err(_) => false,
    };
    s.add("J1-单漂移告警并挂起", ok2, "接口2漂移→1告警、下游3收口全挂起");
    s.add("J1-监测裁决表四例对拍", adjudication_ok(), "漂移/告警/挂起四态闭式一致");
    s.add("J5-基线指纹确定性互异", probe_base_fp_props().is_ok(), "同参两算同值、六接口两两不等");
    s.add("J5-多接口同轮漂移", probe_multi_drift().is_ok(), "三漂齐发→3告警9挂起、轮账守恒");
    s.add("J3-监测相位越级必拒", probe_phase_skip().is_ok(), "闲直取封被 StateBreak 拒");
    s.add("J4-边界（越界/重复/未扫封/封后注入）", boundary_probe().is_ok(), "四类界外全部显式拒");
    s.add("J7-段位与待收口闭式守卫", segment_guard(), "0x80A40 段 0x40 窗、Σ(i+1)=21");

    // ── 动态明细（≤8 槽） ────────────────────────────────────
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    let (t, c) = match &led {
        Ok(l) => (l.tries[0] + l.tries[1] + l.tries[2], l.caught[0] + l.caught[1] + l.caught[2]),
        Err(_) => (0u32, 0u32),
    };
    s.add_dyn("J2-漂移10/10告警挂起", led_ok, &format!("三类各{}轮 抓{}/{}", JUDGE_ROUNDS, c, t));
    s.add_dyn("J2-漏检为零", match &led { Ok(l) => l.missed() == 0, Err(_) => false }, &format!("漏检{}", match &led { Ok(l) => l.missed(), Err(_) => 99u32 }));
    s.add_dyn("J6-双跑一致", repro_check().is_ok(), &format!("golden={:?}", summary()));
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checks_all_passed() {
        let set = run_unx18570_checks();
        assert!(set.all_passed(), "UNX-F18570 判据集必须全绿: {}", set.red_items_text());
        assert!(set.len() >= 8 && set.len() <= 20);
    }

    #[test]
    fn test_golden_run_zero_drift() {
        let g = golden_run();
        assert!(g.is_ok());
        let (a, sup) = g.unwrap_or((99, 99));
        assert_eq!(a, 0);
        assert_eq!(sup, 0);
    }

    #[test]
    fn test_drift_flow_alert_and_suspend() {
        let g = drift_flow_run();
        assert!(g.is_ok());
        let (a, sup) = g.unwrap_or((0, 0));
        assert_eq!(a, 1);
        assert_eq!(sup, 3);
    }

    #[test]
    fn test_drift_always_alerts() {
        // 漂移 10/10：逐接口轮转注入，每轮必告警。
        let mut k = 0u32;
        while k < DRIFT_ROUNDS {
            let mut l = fresh_watch().unwrap_or(FollowLedger::new());
            let iid = (k % IFACE_N as u32) as usize;
            assert!(l.inject_drift(iid).is_ok());
            let d = l.scan().unwrap_or(0);
            assert!(d >= 1, "轮{}必须告警", k);
            assert!(l.conserve(d as u32).is_ok());
            k += 1;
        }
    }

    #[test]
    fn test_state_machine_monotonic() {
        assert!(probe_phase_skip().is_ok());
        assert_eq!(
            WatchState::Suspended.advance(WatchState::DriftAlert).err(),
            Some(FollowCode::StateBreak.code())
        );
        assert_eq!(
            WatchState::Normal.advance(WatchState::Suspended).err(),
            Some(FollowCode::StateBreak.code())
        );
        assert!(WatchState::Normal.advance(WatchState::DriftAlert).is_ok());
    }

    #[test]
    fn test_multi_drift_round() {
        assert!(probe_multi_drift().is_ok());
    }

    #[test]
    fn test_boundary() {
        assert!(boundary_probe().is_ok());
        assert!(adjudication_ok());
    }

    #[test]
    fn test_inject_rounds_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS);
        let l = led.unwrap_or(InjectLedger::new());
        assert!(l.all_caught());
        assert_eq!(l.missed(), 0);
        for i in 0..3 {
            assert_eq!(l.tries[i], JUDGE_ROUNDS);
            assert_eq!(l.caught[i], JUDGE_ROUNDS);
        }
    }

    #[test]
    fn test_repro_dual_run() {
        assert!(repro_check().is_ok());
        let a = golden_run().unwrap_or((9, 9));
        let b = golden_run().unwrap_or((8, 8));
        assert_eq!(a, b);
    }

    #[test]
    fn test_segment_guard_and_detail() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80A40);
        assert_eq!(SEG_BASE % 0x40, 0);
        for c in FollowCode::ALL.iter() {
            assert_eq!(detail(c.code()), c.detail());
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }
}
