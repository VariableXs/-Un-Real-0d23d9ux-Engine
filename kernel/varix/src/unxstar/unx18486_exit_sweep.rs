//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18486 · 进程退出自动清理（覆写零残留） · 0x7F540 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18486-J1 进程退出覆写零残留断言全过（退出后矩阵无悬空条目）
//! 本模块结构清单：
//!   1. 冻结常量层：存活矩阵 16 格 / 进程注册 8 位 / 判据轮数 10；
//!   2. 诊断码闭集 SwCode（code()=SEG_BASE+n，detail() 中文闭集逐码非空）；
//!   3. 数据结构：
//!      - 数组账本 LiveMatrix：[LiveEntry; 16] 存活覆写格（owner+值）；
//!      - 位面 LivePlane：u16 位图，第 i 位=格 i 在场；
//!      - 进程态账 ProcTable：[ProcSlot; 8]，状态机 Running→Exited 单向；
//!      - 退出账 ExitedLog：已退出 pid 流水（守恒对账载体）；
//!   4. 清扫器 sweep(pid)：退出时把该 pid 全部覆写格摘除，摘除后悬空
//!      扫描必须 0 条（零残留判据核心）；
//!   5. 错误路径：幽灵 pid / pid 越界 / 重复退出 / 退出后登记（死进程
//!      写入）/ 悬空残留 / 矩阵满，全部显式拒绝码；
//!   6. 注入核：悬空条目注入 / 重复退出 / 幽灵 pid 退出 / 死进程登记
//!      四类坏样本逐类 10/10 断言被检出；
//!   7. run_unx18486_checks：14 条判据（零残留/守恒/单调/确定性/非法必拒）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段基址（0x7F540，G1 工单 A09 批05 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7F540;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 存活矩阵容量（写死 16）：8 进程 × 2 主格，16=2^4 便于 u16 位面折算；
/// 扩容须换版 v2。
pub const MX_CAP: usize = 16;
/// 进程注册表容量（写死 8）。
pub const PROC_CAP: usize = 8;
/// 进程号域上界（冻结：1..=199，0 保留为内核哨兵）。
pub const PID_MAX: u32 = 199;
/// 判据轮数（写死 10：判据原文"零残留断言全过"取 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 清扫语义版本号（冻结 v1）。
pub const SWEEP_VER_V1: u32 = 1;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwCode {
    /// 幽灵 pid：注册表查无此进程
    GhostPid,
    /// pid 越界：撞保留字 0 或超 199
    PidOutOfRange,
    /// 重复退出：Exited 态再退出
    DupExit,
    /// 死进程登记：Exited 态进程试图再登记覆写
    DeadEnroll,
    /// 悬空残留：退出后矩阵仍有该 pid 条目（零残留违例）
    SweepResidue,
    /// 悬空检出：已退出 pid 的条目仍在场（注入检出路径）
    DanglingDetected,
    /// 矩阵满：16 格用尽
    MatrixFull,
    /// 重复登记：同 (pid,ovr) 已在场
    DupEntry,
    /// 幽灵条目：查无此覆写
    GhostEntry,
    /// 守恒破坏：登记数=在场+清扫+拒收失衡
    ConservationBroken,
    /// 位面失衡：在场位面计数与账本不符
    PlaneMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl SwCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            SwCode::GhostPid => "幽灵 pid：注册表查无此进程",
            SwCode::PidOutOfRange => "pid 越界：撞保留字 0 或超 199",
            SwCode::DupExit => "重复退出：Exited 态再退出，单向机倒退",
            SwCode::DeadEnroll => "死进程登记：Exited 态进程再登记覆写，v1 必拒",
            SwCode::SweepResidue => "悬空残留：退出后矩阵仍有该 pid 条目",
            SwCode::DanglingDetected => "悬空检出：已退出 pid 的条目仍在场",
            SwCode::MatrixFull => "矩阵满：16 格用尽，须升版扩容",
            SwCode::DupEntry => "重复登记：同 (pid,ovr) 已在场",
            SwCode::GhostEntry => "幽灵条目：查无此覆写",
            SwCode::ConservationBroken => "守恒破坏：登记数=在场+清扫+拒收失衡",
            SwCode::PlaneMismatch => "位面失衡：在场位面计数与账本不符",
            SwCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SwCode; 12] = [
        SwCode::GhostPid,
        SwCode::PidOutOfRange,
        SwCode::DupExit,
        SwCode::DeadEnroll,
        SwCode::SweepResidue,
        SwCode::DanglingDetected,
        SwCode::MatrixFull,
        SwCode::DupEntry,
        SwCode::GhostEntry,
        SwCode::ConservationBroken,
        SwCode::PlaneMismatch,
        SwCode::NotReproducible,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < SwCode::ALL.len() {
        if let Some(c) = SwCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F540 段闭集内"
}

// ── 进程状态机（Running→Exited 单向） ──────────────────────────────
/// 进程二态（冻结 v1，只许单向推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcState {
    /// 存活：可登记覆写。
    Running,
    /// 已退出：覆写必须已清扫，禁止一切登记。
    Exited,
}

/// 单进程槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProcSlot {
    pub pid: u32,
    pub state: ProcState,
    /// 该进程历史登记总数（跨清扫累计，守恒对账用）。
    pub enroll_total: u32,
}

impl ProcSlot {
    pub const fn empty() -> ProcSlot {
        ProcSlot { pid: 0, state: ProcState::Running, enroll_total: 0 }
    }
    pub fn is_empty(&self) -> bool {
        self.pid == 0
    }
}

/// 单条存活覆写格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LiveEntry {
    /// 覆写 id（1..=255，0=空槽哨兵）。
    pub ovr_id: u32,
    /// owner 进程号。
    pub owner: u32,
    /// 覆写值。
    pub value: u32,
}

impl LiveEntry {
    pub const fn empty() -> LiveEntry {
        LiveEntry { ovr_id: 0, owner: 0, value: 0 }
    }
    pub fn is_empty(&self) -> bool {
        self.ovr_id == 0
    }
}

// ── 在场位面 ───────────────────────────────────────────────────────
/// 在场位面：16 格用 u16 恰装下。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LivePlane(pub u16);

impl LivePlane {
    pub const fn new() -> LivePlane {
        LivePlane(0)
    }
    pub fn set(&mut self, slot: usize) {
        if slot < MX_CAP {
            self.0 |= 1u16 << slot;
        }
    }
    pub fn clear(&mut self, slot: usize) {
        if slot < MX_CAP {
            self.0 &= !(1u16 << slot);
        }
    }
    pub fn get(self, slot: usize) -> bool {
        if slot < MX_CAP {
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

// ── 清扫引擎 ───────────────────────────────────────────────────────
/// 清扫引擎：进程注册表 + 存活矩阵 + 退出账 + 悬空扫描。
pub struct SweepEngine {
    pub procs: [ProcSlot; PROC_CAP],
    pub matrix: [LiveEntry; MX_CAP],
    pub plane: LivePlane,
    /// 已退出 pid 流水（守恒对账载体）。
    pub exited_log: Vec<u32>,
    /// 登记总数（跨清扫累计）。
    pub enroll_total: u32,
    /// 清扫摘除总数。
    pub swept_total: u32,
    /// 被拒操作总数。
    pub rejected_total: u32,
    /// 清扫轮次（单调）。
    pub sweep_rounds: u32,
    /// 拒因账。
    pub reject_log: Vec<u32>,
}

impl SweepEngine {
    pub fn new() -> SweepEngine {
        SweepEngine {
            procs: [ProcSlot::empty(); PROC_CAP],
            matrix: [LiveEntry::empty(); MX_CAP],
            plane: LivePlane::new(),
            exited_log: Vec::new(),
            enroll_total: 0,
            swept_total: 0,
            rejected_total: 0,
            sweep_rounds: 0,
            reject_log: Vec::new(),
        }
    }

    /// pid 定进程槽。
    fn find_proc(&self, pid: u32) -> Option<usize> {
        let mut i = 0;
        while i < PROC_CAP {
            if let Some(p) = self.procs.get(i) {
                if !p.is_empty() && p.pid == pid {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// (pid,ovr) 定矩阵槽。
    fn find_entry(&self, pid: u32, ovr_id: u32) -> Option<usize> {
        let mut i = 0;
        while i < MX_CAP {
            if let Some(e) = self.matrix.get(i) {
                if !e.is_empty() && e.owner == pid && e.ovr_id == ovr_id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 空进程槽。
    fn free_proc(&self) -> Option<usize> {
        let mut i = 0;
        while i < PROC_CAP {
            if let Some(p) = self.procs.get(i) {
                if p.is_empty() {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 空矩阵槽。
    fn free_entry(&self) -> Option<usize> {
        let mut i = 0;
        while i < MX_CAP {
            if let Some(e) = self.matrix.get(i) {
                if e.is_empty() {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 注册进程。
    pub fn register(&mut self, pid: u32) -> Result<(), u32> {
        if pid == 0 || pid > PID_MAX {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(SwCode::PidOutOfRange.code());
            return Err(SwCode::PidOutOfRange.code());
        }
        if self.find_proc(pid).is_some() {
            // 幂等重注册：已注册即成功（Running 态）。
            return Ok(());
        }
        let slot = match self.free_proc() {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(SwCode::MatrixFull.code());
                return Err(SwCode::MatrixFull.code());
            }
        };
        if let Some(p) = self.procs.get_mut(slot) {
            *p = ProcSlot { pid, state: ProcState::Running, enroll_total: 0 };
        }
        Ok(())
    }

    /// 进程态只读。
    pub fn proc_state(&self, pid: u32) -> Option<ProcState> {
        let slot = self.find_proc(pid)?;
        match self.procs.get(slot) {
            Some(p) => Some(p.state),
            None => None,
        }
    }

    /// 条目在场只读（审计用）。
    pub fn has_entry(&self, pid: u32, ovr_id: u32) -> bool {
        self.find_entry(pid, ovr_id).is_some()
    }

    /// 登记：进程必须 Running。
    pub fn enroll(&mut self, pid: u32, ovr_id: u32, value: u32) -> Result<(), u32> {
        if ovr_id == 0 || ovr_id > 255 {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(SwCode::GhostEntry.code());
            return Err(SwCode::GhostEntry.code());
        }
        let pslot = match self.find_proc(pid) {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(SwCode::GhostPid.code());
                return Err(SwCode::GhostPid.code());
            }
        };
        let state = match self.procs.get(pslot) {
            Some(p) => p.state,
            None => ProcState::Exited,
        };
        if state == ProcState::Exited {
            // 死进程登记必拒（判据注入面）。
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(SwCode::DeadEnroll.code());
            return Err(SwCode::DeadEnroll.code());
        }
        if self.find_entry(pid, ovr_id).is_some() {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(SwCode::DupEntry.code());
            return Err(SwCode::DupEntry.code());
        }
        let slot = match self.free_entry() {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(SwCode::MatrixFull.code());
                return Err(SwCode::MatrixFull.code());
            }
        };
        if let Some(e) = self.matrix.get_mut(slot) {
            *e = LiveEntry { ovr_id, owner: pid, value };
        }
        self.plane.set(slot);
        if let Some(p) = self.procs.get_mut(pslot) {
            p.enroll_total = p.enroll_total.saturating_add(1);
        }
        self.enroll_total = self.enroll_total.saturating_add(1);
        Ok(())
    }

    /// 退出 + 自动清扫：Running→Exited，同轮摘除该 pid 全部覆写格。
    /// 返回摘除格数；摘除后悬空扫描必须 0。
    pub fn exit_and_sweep(&mut self, pid: u32) -> Result<u32, u32> {
        let pslot = match self.find_proc(pid) {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(SwCode::GhostPid.code());
                return Err(SwCode::GhostPid.code());
            }
        };
        let state = match self.procs.get(pslot) {
            Some(p) => p.state,
            None => ProcState::Exited,
        };
        if state == ProcState::Exited {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(SwCode::DupExit.code());
            return Err(SwCode::DupExit.code());
        }
        // 清扫：线性扫矩阵摘除该 pid 全部条目。
        let mut removed = 0u32;
        let mut i = 0;
        while i < MX_CAP {
            let mut hit = false;
            if let Some(e) = self.matrix.get(i) {
                if !e.is_empty() && e.owner == pid {
                    hit = true;
                }
            }
            if hit {
                if let Some(e) = self.matrix.get_mut(i) {
                    *e = LiveEntry::empty();
                }
                self.plane.clear(i);
                removed = removed.saturating_add(1);
                self.swept_total = self.swept_total.saturating_add(1);
            }
            i = i.saturating_add(1);
        }
        // 状态推进 + 退出账。
        if let Some(p) = self.procs.get_mut(pslot) {
            p.state = ProcState::Exited;
        }
        self.exited_log.push(pid);
        self.sweep_rounds = self.sweep_rounds.saturating_add(1);
        // 零残留断言（判据核心）：退出后矩阵不得再有该 pid 条目。
        let residue = self.dangling_scan(pid);
        if residue != 0 {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(SwCode::SweepResidue.code());
            return Err(SwCode::SweepResidue.code());
        }
        Ok(removed)
    }

    /// 悬空扫描：统计"owner 已退出但条目仍在场"的格数。
    pub fn dangling_scan(&self, pid: u32) -> u32 {
        // pid 是否已退出。
        let mut exited = false;
        let mut i = 0;
        while i < self.exited_log.len() {
            if let Some(p) = self.exited_log.get(i) {
                if *p == pid {
                    exited = true;
                    i = self.exited_log.len(); // break
                }
            }
            if !exited {
                i = i.saturating_add(1);
            }
        }
        if !exited {
            return 0;
        }
        let mut n = 0u32;
        let mut j = 0;
        while j < MX_CAP {
            if let Some(e) = self.matrix.get(j) {
                if !e.is_empty() && e.owner == pid {
                    n = n.saturating_add(1);
                }
            }
            j = j.saturating_add(1);
        }
        n
    }

    /// 全局悬空审计：对退出账逐 pid 扫悬空，任何非零即违例。
    pub fn dangling_audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.exited_log.len() {
            if let Some(p) = self.exited_log.get(i) {
                if self.dangling_scan(*p) != 0 {
                    return Err(SwCode::DanglingDetected.code());
                }
            }
            i = i.saturating_add(1);
        }
        Ok(())
    }

    /// 位面对账：在场位面计数 = 非空格数。
    pub fn plane_audit(&self) -> Result<(), u32> {
        let mut n = 0u32;
        let mut i = 0;
        while i < MX_CAP {
            if let Some(e) = self.matrix.get(i) {
                if !e.is_empty() {
                    n = n.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        if self.plane.count() != n {
            return Err(SwCode::PlaneMismatch.code());
        }
        Ok(())
    }

    /// 账目守恒：enroll_total = 在场 + swept_total。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut alive = 0u32;
        let mut i = 0;
        while i < MX_CAP {
            if let Some(e) = self.matrix.get(i) {
                if !e.is_empty() {
                    alive = alive.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        if self.enroll_total != alive.saturating_add(self.swept_total) {
            return Err(SwCode::ConservationBroken.code());
        }
        Ok(())
    }
}

// ── 黄金全链 ───────────────────────────────────────────────────────
/// 黄金全链：4 进程各登记 2 条 → 逐个退出清扫 → 悬空审计 0 → 守恒。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut e = SweepEngine::new();
    // 4 进程 × 2 条。
    let mut p = 0u32;
    while p < 4u32 {
        let pid = 10 + p;
        e.register(pid)?;
        e.enroll(pid, 1 + p * 2, 0x100 + p)?;
        e.enroll(pid, 2 + p * 2, 0x200 + p)?;
        p = p.saturating_add(1);
    }
    e.plane_audit()?;
    // 逐个退出清扫。
    let mut q = 0u32;
    while q < 4u32 {
        let pid = 10 + q;
        let removed = e.exit_and_sweep(pid)?;
        if removed != 2 {
            return Err(SwCode::SweepResidue.code());
        }
        q = q.saturating_add(1);
    }
    e.dangling_audit()?;
    e.plane_audit()?;
    e.conserve()?;
    if e.swept_total != 8 || e.plane.count() != 0 {
        return Err(SwCode::ConservationBroken.code());
    }
    Ok((e.enroll_total, e.swept_total))
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
///   槽 0：悬空条目注入（退出后手工塞回一条该 pid 的覆写，悬空审计必抓）
///   槽 1：重复退出
///   槽 2：幽灵 pid 退出
///   槽 3：死进程登记
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0：悬空条目注入——退出清扫后手工把条目塞回矩阵。
        let mut e0 = SweepEngine::new();
        let _ = e0.register(50);
        let _ = e0.enroll(50, 7, 0x700);
        let _ = e0.exit_and_sweep(50);
        // 人为注入悬空条目（模拟清扫器被绕过）。
        let mut slot: Option<usize> = None;
        let mut i = 0;
        while i < MX_CAP {
            if let Some(x) = e0.matrix.get(i) {
                if x.is_empty() {
                    slot = Some(i);
                    i = MX_CAP;
                }
            }
            if slot.is_none() {
                i = i.saturating_add(1);
            }
        }
        if let Some(s) = slot {
            if let Some(x) = e0.matrix.get_mut(s) {
                *x = LiveEntry { ovr_id: 8, owner: 50, value: 0x800 };
            }
            e0.plane.set(s);
        }
        let hit0 = e0.dangling_audit().is_err();
        led.record(0, hit0);
        // 槽 1：重复退出。
        let mut e1 = SweepEngine::new();
        let _ = e1.register(51);
        let _ = e1.exit_and_sweep(51);
        let hit1 = match e1.exit_and_sweep(51) {
            Err(c) if c == SwCode::DupExit.code() => true,
            _ => false,
        };
        led.record(1, hit1);
        // 槽 2：幽灵 pid 退出。
        let mut e2 = SweepEngine::new();
        let hit2 = match e2.exit_and_sweep(199) {
            Err(c) if c == SwCode::GhostPid.code() => true,
            _ => false,
        };
        led.record(2, hit2);
        // 槽 3：死进程登记。
        let mut e3 = SweepEngine::new();
        let _ = e3.register(52);
        let _ = e3.exit_and_sweep(52);
        let hit3 = match e3.enroll(52, 9, 0x900) {
            Err(c) if c == SwCode::DeadEnroll.code() => true,
            _ => false,
        };
        led.record(3, hit3);
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ───────────────────────────────────────────────────────
/// 探针 A：正例退出清扫全绿。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut e = SweepEngine::new();
    e.register(60)?;
    e.enroll(60, 1, 0x60)?;
    let removed = e.exit_and_sweep(60)?;
    if removed != 1 {
        return Err(SwCode::SweepResidue.code());
    }
    Ok(())
}

/// 探针 B：未注册 pid 退出必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut e = SweepEngine::new();
    e.exit_and_sweep(77).map(|_| ())
}

/// 探针 C：死进程登记必拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut e = SweepEngine::new();
    e.register(78)?;
    e.exit_and_sweep(78)?;
    e.enroll(78, 2, 0x78)
}

// ── 双跑确定性对拍 ─────────────────────────────────────────────────
/// 双跑：同黄金链两次逐字段相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SwCode::NotReproducible.code());
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
    while i < SwCode::ALL.len() {
        if let Some(c) = SwCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 矩阵容量探测：单进程连登 17 条，第 17 条必返矩阵满。
pub fn matrix_capacity_probe() -> Result<(u32, u32), u32> {
    let mut e = SweepEngine::new();
    e.register(90)?;
    let mut filled = 0u32;
    let mut i = 1u32;
    while i <= 17u32 {
        match e.enroll(90, i, 0x900 + i) {
            Ok(()) => filled = filled.saturating_add(1),
            Err(c) if c == SwCode::MatrixFull.code() => {}
            Err(_) => return Err(SwCode::ConservationBroken.code()),
        }
        i = i.saturating_add(1);
    }
    if filled != MX_CAP as u32 {
        return Err(SwCode::ConservationBroken.code());
    }
    e.conserve()?;
    Ok((filled, e.rejected_total))
}

// ── 判据收口 ───────────────────────────────────────────────────────
/// 判据产出（14 条）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链-退出零残留", g.is_ok(), format!("{:?}", g)));
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-悬空注入检出10/10",
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
    // 零残留独立复验：退出后在场数=0 且悬空审计过。
    let mut e = SweepEngine::new();
    let _ = e.register(20);
    let _ = e.enroll(20, 1, 0x20);
    let _ = e.enroll(20, 2, 0x21);
    let sw = e.exit_and_sweep(20);
    let zero_ok = match &sw {
        Ok(n) => *n == 2 && e.plane.count() == 0 && e.dangling_audit().is_ok(),
        Err(_) => false,
    };
    v.push(("J2-退出后矩阵无悬空条目", zero_ok, format!("{:?}", sw)));
    // 位面对账。
    let plane_ok = e.plane_audit().is_ok();
    v.push(("J2-在场位面对账", plane_ok, String::from("plane.count=非空格数")));
    let conserve_ok = e.conserve().is_ok();
    v.push(("J2-账目守恒", conserve_ok, String::from("enroll=在场+清扫")));
    // 单调：退出账只增（重复退出被拒不改账）。
    let lg_before = e.exited_log.len();
    let _ = e.exit_and_sweep(20);
    let mono = e.exited_log.len() == lg_before;
    v.push((
        "J3-退出账单调",
        mono,
        format!("before={} after={}", lg_before, e.exited_log.len()),
    ));
    // 幂等：注册幂等重入。
    let mut e2 = SweepEngine::new();
    let r1 = e2.register(30);
    let r2 = e2.register(30);
    let idem = r1.is_ok() && r2.is_ok();
    v.push(("J3-注册幂等", idem, String::from("重注册同结果")));
    // 矩阵容量。
    let cap = matrix_capacity_probe();
    v.push(("J4-矩阵容量-16满17拒", cap.is_ok(), format!("{:?}", cap)));
    // 边界：pid 0/200 拒，1/199 收。
    let mut e3 = SweepEngine::new();
    let b0 = e3.register(0).err() == Some(SwCode::PidOutOfRange.code());
    let b1 = e3.register(200).err() == Some(SwCode::PidOutOfRange.code());
    let b2 = e3.register(1).is_ok();
    let b3 = e3.register(PID_MAX).is_ok();
    v.push(("J4-pid域边界", b0 && b1 && b2 && b3, format!("0/200拒 1/199收")));
    // 探针三序。
    let pa = probe_seq_a().is_ok();
    let pb = probe_seq_b().is_err();
    let pc = probe_seq_c().is_err();
    v.push(("J4-探针三序", pa && pb && pc, format!("a={} b!={} c!={}", pa, pb, pc)));
    // 双跑一致。
    v.push(("J5-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // 段位守卫。
    v.push(("J5-段位守卫", segment_guard(), String::from("0x7F540 窗口闭集")));
    // 文案闭集。
    let mut det_ok = true;
    let mut k = 0;
    while k < SwCode::ALL.len() {
        if let Some(c) = SwCode::ALL.get(k) {
            if detail_of(c.code()) != c.detail() || c.detail().len() == 0 {
                det_ok = false;
            }
        }
        k += 1;
    }
    v.push(("J5-诊断文案闭集非空", det_ok, format!("codes={}", SwCode::ALL.len())));
    v.push(("J5-判据收口", v.len() + 1 == 15, format!("n={}", v.len() + 1)));
    v
}

/// 探针入口：F18486 进程退出自动清理自检（14 条）。
pub fn run_unx18486_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18486 exit_sweep");
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
        // 正常流：4 进程登记→退出清扫→零残留→守恒。
        assert!(golden_run().is_ok(), "黄金全链应全绿");
    }

    #[test]
    fn sweep_removes_exactly_owner_entries() {
        // 零残留核心：只摘 owner 自己的条目，兄弟条目保留。
        let mut e = SweepEngine::new();
        assert!(e.register(1).is_ok());
        assert!(e.register(2).is_ok());
        assert!(e.enroll(1, 1, 0x11).is_ok());
        assert!(e.enroll(2, 2, 0x22).is_ok());
        let removed = e.exit_and_sweep(1).unwrap_or(0);
        assert!(removed == 1, "应只摘 pid=1 的 1 条");
        assert!(e.proc_state(1) == Some(ProcState::Exited));
        assert!(e.proc_state(2) == Some(ProcState::Running));
        // pid=2 的条目仍在场。
        assert!(e.has_entry(2, 2), "兄弟条目应保留");
        assert!(e.dangling_audit().is_ok(), "悬空审计应过");
    }

    #[test]
    fn dangling_injection_caught() {
        // 注入必抓：悬空条目 10/10。
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.is_ok());
        if let Ok(l) = led {
            assert!(l.all_caught(), "四槽必须全抓");
            assert!(l.missed_total() == 0, "漏检必须为 0");
            assert!(l.caught[0] == JUDGE_ROUNDS, "悬空槽满轮检出");
        }
    }

    #[test]
    fn dead_pid_rejected() {
        // 死进程登记 + 重复退出。
        let mut e = SweepEngine::new();
        assert!(e.register(5).is_ok());
        assert!(e.exit_and_sweep(5).is_ok());
        assert!(e.enroll(5, 3, 1).err() == Some(SwCode::DeadEnroll.code()));
        assert!(e.exit_and_sweep(5).err() == Some(SwCode::DupExit.code()));
    }

    #[test]
    fn ghost_and_bounds() {
        // 幽灵 pid + 域边界四类。
        let mut e = SweepEngine::new();
        assert!(e.exit_and_sweep(66).err() == Some(SwCode::GhostPid.code()));
        assert!(e.enroll(66, 1, 1).err() == Some(SwCode::GhostPid.code()));
        assert!(e.register(0).err() == Some(SwCode::PidOutOfRange.code()));
        assert!(e.register(200).err() == Some(SwCode::PidOutOfRange.code()));
        assert!(e.register(1).is_ok());
        assert!(e.register(PID_MAX).is_ok());
    }

    #[test]
    fn matrix_capacity_boundary() {
        // 矩阵容量：16 满、17 拒。
        let cap = matrix_capacity_probe();
        assert!(cap.is_ok());
        if let Ok((filled, _r)) = cap {
            assert!(filled == MX_CAP as u32);
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
        let set = run_unx18486_checks();
        assert!(set.all_passed(), "15 条判据应全绿");
        assert!(set.len() == 15, "判据条数应冻结 15");
    }
}
