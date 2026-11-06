//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18483 · 覆写生效范围（当前进程独占） · 0x7F480 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18483-J1 覆写仅本进程可见断言全过（兄弟进程双查对照），
//!       泄漏注入检出
//! 本模块结构清单：
//!   1. 冻结常量层：隔离账本容量 12 / 进程对（P0,P1）/ 判据轮数 10；
//!   2. 诊断码闭集 ScCode（code()=SEG_BASE+n，detail() 中文闭集逐码非空）；
//!   3. 数据结构：
//!      - 数组账本 ScopeLedger：[ScopeEntry; 12]，每条挂 owner 进程号；
//!      - 双视点位面 ViewPlaneA/ViewPlaneB：u16 位图各自记账"该视点可见"；
//!      - 状态机 LeakState：Sealed→LeakFlagged→Quarantined 三态单向；
//!   4. 错误路径：幽灵条目 / 跨视点可见（泄漏）/ 隔离重复 / 位面失衡
//!      / 账目守恒破坏，全部显式拒绝码；
//!   5. 注入核：泄漏（他进程视点出现他人覆写）/ 幽灵查询 / 隔离后复活
//!      / 位面手改 四类坏样本逐类 10/10 断言被检出；
//!   6. 兄弟进程双查对照：owner 查得覆写值、兄弟查得下级源缺省值，
//!      两对照行逐对记录；
//!   7. run_unx18483_checks：14 条判据（隔离/守恒/对照/确定性/非法必拒）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段基址（0x7F480，G1 工单 A09 批05 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7F480;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 隔离账本容量（写死 12）：6 条 owner 覆写 × 2 个对照视点，12=6×2
/// 便于按对折校验；扩容须换版 v2。
pub const SCOPE_CAP: usize = 12;
/// 兄弟进程对（冻结样例：P0=11，P1=12，同为父进程 1 之下）。
pub const SIBLING_P0: u32 = 11;
pub const SIBLING_P1: u32 = 12;
/// 兄弟共享父进程号（对照语义锚）。
pub const PARENT_PID: u32 = 1;
/// 判据轮数（写死 10：判据原文"泄漏注入检出"取 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 隔离语义版本号（冻结 v1）。
pub const SCOPE_VER_V1: u32 = 1;
/// 兄弟缺省值（P1 视点应见到的"下级源缺省"，写死 0xFFF0）。
pub const SIB_FALLBACK: u32 = 0xFFF0;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    /// 幽灵条目：账本查无此覆写
    GhostEntry,
    /// 泄漏：他进程视点出现他人覆写（v1 核心违例）
    CrossViewLeak,
    /// 隔离重复：已隔离条目再隔离
    DupQuarantine,
    /// 复活：隔离态条目再可见
    ReviveAfterQuarantine,
    /// 位面失衡：可见性位面与账本不符
    PlaneMismatch,
    /// 账目守恒破坏：登记数≠可见+隔离+拒收
    ConservationBroken,
    /// 对照破坏：兄弟视点查得非缺省值
    SiblingControlBroken,
    /// 进程号越界：撞保留字 0 或超闭集
    PidOutOfRange,
    /// 双跑不一致
    NotReproducible,
    /// 检漏器失明：注入泄漏未被抓
    DetectorBlind,
    /// 对照表越界：视点编号非 0/1
    ViewOutOfRange,
    /// 空账本建账冻结：空账本不许冻结快照
    FreezeEmpty,
}

impl ScCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            ScCode::GhostEntry => "幽灵条目：账本查无此覆写",
            ScCode::CrossViewLeak => "泄漏：他进程视点出现他人覆写，v1 必检出",
            ScCode::DupQuarantine => "隔离重复：已隔离条目再隔离",
            ScCode::ReviveAfterQuarantine => "复活：隔离态条目再可见，单向机倒退",
            ScCode::PlaneMismatch => "位面失衡：可见性位面与账本不符",
            ScCode::ConservationBroken => "账目守恒破坏：登记数≠可见+隔离+拒收",
            ScCode::SiblingControlBroken => "对照破坏：兄弟视点查得非缺省值",
            ScCode::PidOutOfRange => "进程号越界：撞保留字 0 或超闭集",
            ScCode::NotReproducible => "双跑不一致：复现红线",
            ScCode::DetectorBlind => "检漏器失明：注入泄漏未被抓",
            ScCode::ViewOutOfRange => "对照表越界：视点编号非 0/1",
            ScCode::FreezeEmpty => "空账本不许冻结快照",
        }
    }
    pub const ALL: [ScCode; 12] = [
        ScCode::GhostEntry,
        ScCode::CrossViewLeak,
        ScCode::DupQuarantine,
        ScCode::ReviveAfterQuarantine,
        ScCode::PlaneMismatch,
        ScCode::ConservationBroken,
        ScCode::SiblingControlBroken,
        ScCode::PidOutOfRange,
        ScCode::NotReproducible,
        ScCode::DetectorBlind,
        ScCode::ViewOutOfRange,
        ScCode::FreezeEmpty,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < ScCode::ALL.len() {
        if let Some(c) = ScCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F480 段闭集内"
}

// ── 泄漏状态机（Sealed→LeakFlagged→Quarantined 单向） ──────────────
/// 隔离三态（冻结 v1，只许单向推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeakState {
    /// 密封：仅 owner 视点可见。
    Sealed,
    /// 已标记：检漏器发现跨视点泄漏，待隔离。
    LeakFlagged,
    /// 已隔离：泄漏已拦截，终态。
    Quarantined,
}

/// 单条隔离账条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScopeEntry {
    /// 覆写 id（1..=64，0=空槽哨兵）。
    pub ovr_id: u32,
    /// owner 进程号（覆写仅此进程可见）。
    pub owner: u32,
    /// 覆写值（owner 视点应查得的值）。
    pub value: u32,
    /// 泄漏状态。
    pub state: LeakState,
    /// 检漏计数（对照扫描轮数，单调）。
    pub sweep_ticks: u16,
}

impl ScopeEntry {
    pub const fn empty() -> ScopeEntry {
        ScopeEntry { ovr_id: 0, owner: 0, value: 0, state: LeakState::Sealed, sweep_ticks: 0 }
    }
    pub fn is_empty(&self) -> bool {
        self.ovr_id == 0
    }
}

// ── 视点位面：第 i 位=槽 i 在该视点"可见" ──────────────────────────
/// 单视点位面（P0/P1 各一份）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ViewPlane(pub u16);

impl ViewPlane {
    pub const fn new() -> ViewPlane {
        ViewPlane(0)
    }
    pub fn set(&mut self, slot: usize) {
        if slot < SCOPE_CAP {
            self.0 |= 1u16 << slot;
        }
    }
    pub fn clear(&mut self, slot: usize) {
        if slot < SCOPE_CAP {
            self.0 &= !(1u16 << slot);
        }
    }
    pub fn get(self, slot: usize) -> bool {
        if slot < SCOPE_CAP {
            (self.0 >> slot) & 1u16 == 1u16
        } else {
            false
        }
    }
    pub fn count(self) -> u32 {
        let mut n = 0u32;
        let mut b = self.0;
        while b != 0 {
            if b & 1u16 == 1u16 {
                n = n.saturating_add(1);
            }
            b >>= 1;
        }
        n
    }
}

// ── 隔离账本 + 双视点对照 ──────────────────────────────────────────
/// 隔离账本：登记 owner 覆写，维护 P0/P1 双视点位面，检漏扫描。
pub struct ScopeLedger {
    pub slots: [ScopeEntry; SCOPE_CAP],
    /// P0 视点位面。
    pub plane_p0: ViewPlane,
    /// P1 视点位面。
    pub plane_p1: ViewPlane,
    /// 登记总数。
    pub enrolled_total: u32,
    /// 已隔离总数。
    pub quarantined_total: u32,
    /// 被拒操作总数。
    pub rejected_total: u32,
    /// 对照轮次累加器（单调）。
    pub control_rounds: u32,
    /// 拒因账。
    pub reject_log: Vec<u32>,
}

impl ScopeLedger {
    pub fn new() -> ScopeLedger {
        ScopeLedger {
            slots: [ScopeEntry::empty(); SCOPE_CAP],
            plane_p0: ViewPlane::new(),
            plane_p1: ViewPlane::new(),
            enrolled_total: 0,
            quarantined_total: 0,
            rejected_total: 0,
            control_rounds: 0,
            reject_log: Vec::new(),
        }
    }

    /// 按 ovr_id 线性扫槽。
    fn find_slot(&self, ovr_id: u32) -> Option<usize> {
        let mut i = 0;
        while i < SCOPE_CAP {
            if let Some(e) = self.slots.get(i) {
                if !e.is_empty() && e.ovr_id == ovr_id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 首个空槽。
    fn free_slot(&self) -> Option<usize> {
        let mut i = 0;
        while i < SCOPE_CAP {
            if let Some(e) = self.slots.get(i) {
                if e.is_empty() {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记：owner 视点置位、兄弟视点保持清零——这是"仅本进程可见"的
    /// 位面级落地。重复登记/越界/表满显式拒绝。
    pub fn enroll(&mut self, ovr_id: u32, owner: u32, value: u32) -> Result<(), u32> {
        if ovr_id == 0 || ovr_id > 64 {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(ScCode::GhostEntry.code());
            return Err(ScCode::GhostEntry.code());
        }
        if owner == 0 || owner > 999 {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(ScCode::PidOutOfRange.code());
            return Err(ScCode::PidOutOfRange.code());
        }
        if self.find_slot(ovr_id).is_some() {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(ScCode::GhostEntry.code());
            return Err(ScCode::GhostEntry.code());
        }
        let slot = match self.free_slot() {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(ScCode::ConservationBroken.code());
                return Err(ScCode::ConservationBroken.code());
            }
        };
        if let Some(e) = self.slots.get_mut(slot) {
            *e = ScopeEntry {
                ovr_id,
                owner,
                value,
                state: LeakState::Sealed,
                sweep_ticks: 0,
            };
        }
        // 位面落地：owner 视点可见，兄弟视点不可见。
        if owner == SIBLING_P0 {
            self.plane_p0.set(slot);
        } else if owner == SIBLING_P1 {
            self.plane_p1.set(slot);
        }
        self.enrolled_total = self.enrolled_total.saturating_add(1);
        Ok(())
    }

    /// 视点查询：view=0→P0，view=1→P1。仅 owner 视点返回覆写值；
    /// 兄弟视点返回缺省 0xFFF0（对照语义）。
    pub fn view_query(&self, view: u8, ovr_id: u32) -> Result<u32, u32> {
        let plane = match view {
            0 => self.plane_p0,
            1 => self.plane_p1,
            _ => return Err(ScCode::ViewOutOfRange.code()),
        };
        let slot = match self.find_slot(ovr_id) {
            Some(s) => s,
            None => return Err(ScCode::GhostEntry.code()),
        };
        let (owner, value, state) = match self.slots.get(slot) {
            Some(e) => (e.owner, e.value, e.state),
            None => return Err(ScCode::GhostEntry.code()),
        };
        if state == LeakState::Quarantined {
            // 隔离后复活探测点：隔离条目在 owner 视点也须隐身。
            return Err(ScCode::ReviveAfterQuarantine.code());
        }
        if plane.get(slot) && owner == SIBLING_P0 + view as u32 {
            Ok(value)
        } else {
            Ok(SIB_FALLBACK)
        }
    }

    /// 检漏扫描：扫兄弟视点位面上是否出现非己条目；出现即标记 LeakFlagged。
    /// 返回本轮检出的泄漏槽数。
    pub fn leak_sweep(&mut self) -> u32 {
        let mut found = 0u32;
        let mut i = 0;
        while i < SCOPE_CAP {
            let mut hit = false;
            if let Some(e) = self.slots.get(i) {
                if e.is_empty() || e.state != LeakState::Sealed {
                    i = i.saturating_add(1);
                    continue;
                }
                // P0 位面出现 P1 条目，或 P1 位面出现 P0 条目 = 泄漏。
                if e.owner == SIBLING_P0 && self.plane_p1.get(i) {
                    hit = true;
                }
                if e.owner == SIBLING_P1 && self.plane_p0.get(i) {
                    hit = true;
                }
            }
            if hit {
                if let Some(e) = self.slots.get_mut(i) {
                    e.state = LeakState::LeakFlagged;
                    e.sweep_ticks = e.sweep_ticks.saturating_add(1);
                }
                found = found.saturating_add(1);
            }
            i = i.saturating_add(1);
        }
        self.control_rounds = self.control_rounds.saturating_add(1);
        found
    }

    /// 隔离：LeakFlagged→Quarantined，终态；隔离后视点位面清零。
    pub fn quarantine(&mut self, ovr_id: u32) -> Result<(), u32> {
        let slot = match self.find_slot(ovr_id) {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(ScCode::GhostEntry.code());
                return Err(ScCode::GhostEntry.code());
            }
        };
        let state = match self.slots.get(slot) {
            Some(e) => e.state,
            None => LeakState::Quarantined,
        };
        let new_state = match state {
            LeakState::Sealed => {
                // 未标记即隔离 = 乱序，v1 拒绝（状态机纪律）。
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(ScCode::DupQuarantine.code());
                return Err(ScCode::DupQuarantine.code());
            }
            LeakState::LeakFlagged => LeakState::Quarantined,
            LeakState::Quarantined => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(ScCode::DupQuarantine.code());
                return Err(ScCode::DupQuarantine.code());
            }
        };
        if let Some(e) = self.slots.get_mut(slot) {
            e.state = new_state;
        }
        self.plane_p0.clear(slot);
        self.plane_p1.clear(slot);
        self.quarantined_total = self.quarantined_total.saturating_add(1);
        Ok(())
    }

    /// 位面对账：owner 视点置位数 = 未隔离的 owner 条目数。
    pub fn plane_audit(&self) -> Result<(), u32> {
        let mut expect_p0 = 0u32;
        let mut expect_p1 = 0u32;
        let mut i = 0;
        while i < SCOPE_CAP {
            if let Some(e) = self.slots.get(i) {
                if !e.is_empty() && e.state != LeakState::Quarantined {
                    if e.owner == SIBLING_P0 {
                        expect_p0 = expect_p0.saturating_add(1);
                    } else if e.owner == SIBLING_P1 {
                        expect_p1 = expect_p1.saturating_add(1);
                    }
                }
            }
            i += 1;
        }
        if self.plane_p0.count() != expect_p0 || self.plane_p1.count() != expect_p1 {
            return Err(ScCode::PlaneMismatch.code());
        }
        Ok(())
    }

    /// 账目守恒：enrolled = 存活 + quarantined。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut alive = 0u32;
        let mut i = 0;
        while i < SCOPE_CAP {
            if let Some(e) = self.slots.get(i) {
                if !e.is_empty() && e.state != LeakState::Quarantined {
                    alive = alive.saturating_add(1);
                }
            }
            i += 1;
        }
        if self.enrolled_total != alive.saturating_add(self.quarantined_total) {
            return Err(ScCode::ConservationBroken.code());
        }
        Ok(())
    }
}

// ── 兄弟进程双查对照（判据核心断言载体） ───────────────────────────
/// 双查对照账：owner 值 / 兄弟缺省值逐对记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ControlPair {
    pub ovr_id: u32,
    /// owner 视点查得值。
    pub owner_seen: u32,
    /// 兄弟视点查得值（应为 0xFFF0 缺省）。
    pub sibling_seen: u32,
}

impl ControlPair {
    /// 对照成立：owner 查得覆写值且兄弟查得缺省。
    pub fn holds(&self, want: u32) -> bool {
        self.owner_seen == want && self.sibling_seen == SIB_FALLBACK
    }
}

/// 全账对照：逐条登记双查对照对，全对成立才返回 Ok(对数)。
pub fn sibling_double_check(led: &ScopeLedger) -> Result<Vec<ControlPair>, u32> {
    let mut pairs: Vec<ControlPair> = Vec::new();
    let mut i = 0;
    while i < SCOPE_CAP {
            if let Some(e) = led.slots.get(i) {
                if !e.is_empty() && e.state == LeakState::Sealed {
                    // owner 视点=own，兄弟视点=1-own（双查对照核心）。
                    let own = if e.owner == SIBLING_P0 { 0u8 } else { 1u8 };
                    let ov = led.view_query(own, e.ovr_id);
                    let sv = led.view_query(1 - own, e.ovr_id);
                    let owner_seen = match ov {
                        Ok(v) => v,
                        Err(_) => return Err(ScCode::SiblingControlBroken.code()),
                    };
                    let sibling_seen = match sv {
                        Ok(v) => v,
                        Err(_) => return Err(ScCode::SiblingControlBroken.code()),
                    };
                    let p = ControlPair { ovr_id: e.ovr_id, owner_seen, sibling_seen };
                    if !p.holds(e.value) {
                        return Err(ScCode::SiblingControlBroken.code());
                    }
                    pairs.push(p);
                }
            }
        i = i.saturating_add(1);
    }
    if pairs.len() == 0 {
        return Err(ScCode::FreezeEmpty.code());
    }
    Ok(pairs)
}

// ── 黄金全链 ───────────────────────────────────────────────────────
/// 黄金全链：P0 登记 6 条 → 双查对照全过 → 检漏 0 泄漏 → 守恒。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut led = ScopeLedger::new();
    let mut i = 0u32;
    while i < 6 {
        let ovr = 1 + i;
        let val = 0x1000 + i;
        led.enroll(ovr, SIBLING_P0, val)?;
        i = i.saturating_add(1);
    }
    let pairs = sibling_double_check(&led)?;
    if pairs.len() != 6 {
        return Err(ScCode::SiblingControlBroken.code());
    }
    let leaked = led.leak_sweep();
    if leaked != 0 {
        return Err(ScCode::CrossViewLeak.code());
    }
    led.plane_audit()?;
    led.conserve()?;
    Ok((led.enrolled_total, pairs.len() as u32))
}

// ── 注入检出账 ─────────────────────────────────────────────────────
/// 四槽注入账。
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            if let Some(t) = self.tries.get_mut(slot) {
                *t = t.saturating_add(1);
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed_total(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 注入核：四类坏样本逐类 10/10 断言被检出。
///   槽 0：泄漏注入——把 P0 的覆写手工置入 P1 位面，检漏器必须抓到
///   槽 1：幽灵查询（账外 id）
///   槽 2：乱序隔离（Sealed 未标记即隔离）
///   槽 3：隔离后复活（Quarantined 后视点查询必拒）
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0：泄漏注入（模拟位面被污染）。
        let mut l0 = ScopeLedger::new();
        let _ = l0.enroll(1, SIBLING_P0, 0x2000);
        l0.plane_p1.set(0); // 人为泄漏
        let hit0 = l0.leak_sweep() == 1;
        led.record(0, hit0);
        // 槽 1：幽灵查询。
        let mut l1 = ScopeLedger::new();
        let _ = l1.enroll(2, SIBLING_P0, 0x2001);
        let hit1 = match l1.view_query(0, 99) {
            Err(c) if c == ScCode::GhostEntry.code() => true,
            _ => false,
        };
        led.record(1, hit1);
        // 槽 2：乱序隔离。
        let mut l2 = ScopeLedger::new();
        let _ = l2.enroll(3, SIBLING_P0, 0x2002);
        let hit2 = match l2.quarantine(3) {
            Err(c) if c == ScCode::DupQuarantine.code() => true,
            _ => false,
        };
        led.record(2, hit2);
        // 槽 3：隔离后复活。
        let mut l3 = ScopeLedger::new();
        let _ = l3.enroll(4, SIBLING_P0, 0x2003);
        let _ = l3.leak_sweep();
        l3.plane_p1.set(0); // 人为泄漏：P0 条目出现在 P1 位面（槽 0）
        let _ = l3.leak_sweep(); // 标记 LeakFlagged
        let _ = l3.quarantine(4);
        let hit3 = match l3.view_query(0, 4) {
            Err(c) if c == ScCode::ReviveAfterQuarantine.code() => true,
            _ => false,
        };
        led.record(3, hit3);
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ───────────────────────────────────────────────────────
/// 探针 A：正例对照全绿。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut led = ScopeLedger::new();
    led.enroll(5, SIBLING_P0, 0x3000)?;
    let pairs = sibling_double_check(&led)?;
    if pairs.len() != 1 {
        return Err(ScCode::SiblingControlBroken.code());
    }
    Ok(())
}

/// 探针 B：泄漏必检出。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut led = ScopeLedger::new();
    let _ = led.enroll(6, SIBLING_P1, 0x3001);
    led.plane_p0.set(0);
    if led.leak_sweep() == 0 {
        return Err(ScCode::DetectorBlind.code());
    }
    Ok(())
}

/// 探针 C：视点编号越界必拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut led = ScopeLedger::new();
    let _ = led.enroll(7, SIBLING_P0, 0x3002);
    led.view_query(7, 7).map(|_| ())
}

// ── 双跑确定性对拍 ─────────────────────────────────────────────────
/// 双跑：同黄金链两次逐字段相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ScCode::NotReproducible.code());
    }
    Ok(())
}

// ── 段位守卫 ───────────────────────────────────────────────────────
/// 段位守卫：SEG_BASE 对齐且全部诊断码落段内 0x40 窗口。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < ScCode::ALL.len() {
        if let Some(c) = ScCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 容量耗尽探测：连登记 13 条，第 13 条必拒且账不丢。
pub fn capacity_probe() -> Result<(u32, u32), u32> {
    let mut led = ScopeLedger::new();
    let mut filled = 0u32;
    let mut i = 1u32;
    while i <= 13u32 {
        match led.enroll(i, SIBLING_P0, 0x4000 + i) {
            Ok(()) => filled = filled.saturating_add(1),
            Err(_) => {}
        }
        i = i.saturating_add(1);
    }
    if filled != SCOPE_CAP as u32 {
        return Err(ScCode::ConservationBroken.code());
    }
    led.conserve()?;
    Ok((filled, led.rejected_total))
}

// ── 判据收口 ───────────────────────────────────────────────────────
/// 判据产出（14 条）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链-双查对照全过", g.is_ok(), format!("{:?}", g)));
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-泄漏注入检出10/10",
        inj_ok,
        format!(
            "tries={:?} caught={:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    let miss = match &inj {
        Ok(l) => l.missed_total(),
        Err(_) => 1,
    };
    v.push(("J1-注入守恒漏检零", miss == 0, format!("missed={}", miss)));
    // 双查对照独立复验（判据核心：兄弟双查对照）。
    let mut led = ScopeLedger::new();
    let _ = led.enroll(9, SIBLING_P0, 0x5000);
    let _ = led.enroll(10, SIBLING_P1, 0x5001);
    let ctrl = sibling_double_check(&led);
    let ctrl_ok = match &ctrl {
        Ok(p) => p.len() == 2,
        Err(_) => false,
    };
    v.push(("J2-兄弟双查对照成立", ctrl_ok, format!("{:?}", ctrl.is_ok())));
    // 位面对账。
    let plane_ok = led.plane_audit().is_ok();
    v.push(("J2-双视点位面对账", plane_ok, String::from("P0/P1 位面=未隔离条目")));
    let conserve_ok = led.conserve().is_ok();
    v.push(("J2-账目守恒", conserve_ok, String::from("enrolled=alive+隔离")));
    // 单调：隔离只进不出。
    let mut l2 = ScopeLedger::new();
    let _ = l2.enroll(11, SIBLING_P0, 0x5002);
    let _ = l2.leak_sweep();
    l2.plane_p1.set(0);
    let _ = l2.leak_sweep();
    let _ = l2.quarantine(11);
    let qv = l2.quarantined_total;
    let _ = l2.quarantine(11);
    let mono = l2.quarantined_total == qv;
    v.push(("J3-隔离单调-终态不重入", mono, format!("before={} after={}", qv, l2.quarantined_total)));
    // 容量耗尽。
    let cap = capacity_probe();
    v.push(("J4-容量耗尽-12满13拒", cap.is_ok(), format!("{:?}", cap)));
    // 边界：ovr 0/65 拒，1/64 收；pid 0 拒。
    let mut l3 = ScopeLedger::new();
    let b0 = l3.enroll(0, SIBLING_P0, 1).is_err();
    let b1 = l3.enroll(65, SIBLING_P0, 1).is_err();
    let b2 = l3.enroll(1, SIBLING_P0, 1).is_ok();
    let b3 = l3.enroll(64, SIBLING_P1, 1).is_ok();
    let b4 = l3.enroll(63, 0, 1).is_err();
    v.push((
        "J4-域边界五类",
        b0 && b1 && b2 && b3 && b4,
        format!("0拒{} 65拒{} 1收{} 64收{} pid0拒{}", b0, b1, b2, b3, b4),
    ));
    // 探针三序。
    let pa = probe_seq_a().is_ok();
    let pb = probe_seq_b().is_ok();
    let pc = probe_seq_c().is_err();
    v.push(("J4-探针三序", pa && pb && pc, format!("a={} b={} c!={}", pa, pb, pc)));
    // 双跑一致。
    v.push(("J5-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // 段位守卫。
    v.push(("J5-段位守卫", segment_guard(), String::from("0x7F480 窗口闭集")));
    // 文案闭集。
    let mut det_ok = true;
    let mut k = 0;
    while k < ScCode::ALL.len() {
        if let Some(c) = ScCode::ALL.get(k) {
            if detail_of(c.code()) != c.detail() || c.detail().len() == 0 {
                det_ok = false;
            }
        }
        k += 1;
    }
    v.push(("J5-诊断文案闭集非空", det_ok, format!("codes={}", ScCode::ALL.len())));
    v.push(("J5-判据收口", v.len() + 1 == 14, format!("n={}", v.len() + 1)));
    v
}

/// 探针入口：F18483 覆写生效范围自检（14 条）。
pub fn run_unx18483_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18483 ovr_scope");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i = i.saturating_add(1);
    }
    s
}

// ── 测试（8 个） ───────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_flow_golden_chain() {
        // 正常流：登记→双查对照→检漏 0→守恒。
        assert!(golden_run().is_ok(), "黄金全链应全绿");
    }

    #[test]
    fn sibling_isolation_strict() {
        // 隔离核心：P0 覆写在 P0 视点见真值、P1 视点见缺省；反向亦然。
        let mut led = ScopeLedger::new();
        assert!(led.enroll(1, SIBLING_P0, 0xA0A0).is_ok());
        assert!(led.enroll(2, SIBLING_P1, 0xB0B0).is_ok());
        // P0 视点：自己的 1 真值，兄弟的 2 缺省。
        assert!(led.view_query(0, 1).err().is_none());
        assert!(led.view_query(0, 1) == Ok(0xA0A0));
        assert!(led.view_query(0, 2) == Ok(SIB_FALLBACK));
        // P1 视点：自己的 2 真值，兄弟的 1 缺省。
        assert!(led.view_query(1, 2) == Ok(0xB0B0));
        assert!(led.view_query(1, 1) == Ok(SIB_FALLBACK));
    }

    #[test]
    fn leak_injection_caught() {
        // 注入必抓：泄漏注入 10/10。
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.is_ok());
        if let Ok(l) = led {
            assert!(l.all_caught(), "四槽必须全抓");
            assert!(l.missed_total() == 0, "漏检必须为 0");
            assert!(l.caught[0] == JUDGE_ROUNDS, "泄漏槽满轮检出");
        }
    }

    #[test]
    fn quarantine_order_enforced() {
        // 状态机：Sealed 直接隔离=乱序拒；LeakFlagged→Quarantined=合法。
        let mut led = ScopeLedger::new();
        assert!(led.enroll(3, SIBLING_P0, 7).is_ok());
        assert!(led.quarantine(3).err() == Some(ScCode::DupQuarantine.code()));
        led.plane_p1.set(0);
        assert!(led.leak_sweep() == 1, "泄漏应被标记");
        assert!(led.quarantine(3).is_ok(), "标记后隔离应过");
        assert!(led.quarantine(3).err() == Some(ScCode::DupQuarantine.code()));
    }

    #[test]
    fn ghost_and_view_bounds() {
        // 幽灵条目与视点编号越界。
        let mut led = ScopeLedger::new();
        let _ = led.enroll(4, SIBLING_P0, 1);
        assert!(led.view_query(0, 99).err() == Some(ScCode::GhostEntry.code()));
        assert!(led.view_query(2, 4).err() == Some(ScCode::ViewOutOfRange.code()));
        assert!(led.view_query(255, 4).err() == Some(ScCode::ViewOutOfRange.code()));
    }

    #[test]
    fn capacity_exhaustion_boundary() {
        // 容量边界：12 满、13 拒。
        let cap = capacity_probe();
        assert!(cap.is_ok());
        if let Ok((filled, _r)) = cap {
            assert!(filled == SCOPE_CAP as u32);
        }
    }

    #[test]
    fn double_run_deterministic() {
        // 双跑一致。
        assert!(repro_check().is_ok(), "双跑应逐字段一致");
    }

    #[test]
    fn segment_guard_and_checkset() {
        // 段位守卫 + 判据收口全绿。
        assert!(segment_guard(), "段位守卫应过");
        let set = run_unx18483_checks();
        assert!(set.all_passed(), "14 条判据应全绿");
        assert!(set.len() == 14, "判据条数应冻结 14");
    }
}
