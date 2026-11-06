//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18482 · 进程内临时覆写创建语义（句柄绑定） · 0x7F440 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18482-J1 覆写创建绑定进程句柄断言全过，跨进程覆写注入
//!       10/10 拒绝
//! 本模块结构清单：
//!   1. 冻结常量层：句柄注册表容量 12 / 句柄域 1..=4096 / 判据轮数 10；
//!   2. 诊断码闭集 HbCode（code()=SEG_BASE+n，detail() 中文闭集逐码非空）；
//!   3. 数据结构：
//!      - 数组账本 HandleTable：[HandleSlot; 12] 定容句柄表；
//!      - 位面 BindPlane：u16 位图，第 i 位=槽 i 已被覆写绑定；
//!      - 状态机 SlotState：Free→Bound→Retired 三态单向；
//!   4. 错误路径：跨进程（owner 不符）/ 幽灵句柄 / 重复绑定 / 已退役
//!      再绑 / 句柄域越界 / 表满 六类显式拒绝；
//!   5. 注入核：跨进程覆写 / 幽灵句柄 / 重复绑定 / 退役再绑 四类坏样本
//!      逐类 10/10 断言被显式拒绝，计数守恒漏检必 0；
//!   6. 黄金全链 golden_run：6 进程各建 1 句柄并绑覆写，全链走通+双跑
//!      逐字段一致；
//!   7. run_unx18482_checks：14 条判据（绑定/守恒/幂等/确定性/非法必拒）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段基址（0x7F440，G1 工单 A09 批05 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7F440;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 句柄表容量（写死 12）：6 个样板进程 × (1 主句柄+1 备份槽)，12 恰为
/// 2 的幂便于位面折算；扩容须换版 v2。
pub const HANDLE_CAP: usize = 12;
/// 句柄值域上界（冻结：1..=4096，0 保留为"空槽哨兵"）。
pub const HANDLE_ID_MAX: u32 = 4096;
/// 进程号域上界（冻结：1..=999，0 保留为内核哨兵）。
pub const PID_MAX: u32 = 999;
/// 判据轮数（写死 10：判据原文"跨进程覆写注入 10/10 拒绝"取 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 绑定语义版本号（冻结 v1）。
pub const BIND_VER_V1: u32 = 1;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HbCode {
    /// 幽灵句柄：表中查无此句柄
    HandleNotFound,
    /// 跨进程覆写：owner 与请求进程不符
    OwnerMismatch,
    /// 重复绑定：句柄已绑覆写且未退役
    AlreadyBound,
    /// 已退役：退役槽不许再绑
    SlotRetired,
    /// 句柄越界：超出 1..=4096 或撞保留字 0
    HandleOutOfRange,
    /// 进程号越界：超出 1..=999 或撞保留字 0
    PidOutOfRange,
    /// 表满：12 槽用尽
    TableFull,
    /// 幽灵进程：请求进程未在表注册
    GhostPid,
    /// 绑定守恒破坏：绑定数≠位面计数
    BindPlaneMismatch,
    /// 账目守恒破坏：注册数≠占用+退役+拒收
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
    /// 幂等破约：同参数二次注册应得同结果而未得
    IdempotenceBroken,
}

impl HbCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            HbCode::HandleNotFound => "幽灵句柄：表中查无此句柄",
            HbCode::OwnerMismatch => "跨进程覆写：句柄 owner 与请求进程不符，v1 必拒",
            HbCode::AlreadyBound => "重复绑定：句柄已绑覆写且未退役",
            HbCode::SlotRetired => "已退役：退役槽不许再绑新覆写",
            HbCode::HandleOutOfRange => "句柄越界：超出 1..=4096 或撞保留字 0",
            HbCode::PidOutOfRange => "进程号越界：超出 1..=999 或撞保留字 0",
            HbCode::TableFull => "表满：12 槽用尽，须升版扩容",
            HbCode::GhostPid => "幽灵进程：请求进程未注册",
            HbCode::BindPlaneMismatch => "绑定守恒破坏：绑定数≠位面计数",
            HbCode::ConservationBroken => "账目守恒破坏：注册数≠占用+退役+拒收",
            HbCode::NotReproducible => "双跑不一致：复现红线",
            HbCode::IdempotenceBroken => "幂等破约：同参数二次注册结果漂移",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [HbCode; 12] = [
        HbCode::HandleNotFound,
        HbCode::OwnerMismatch,
        HbCode::AlreadyBound,
        HbCode::SlotRetired,
        HbCode::HandleOutOfRange,
        HbCode::PidOutOfRange,
        HbCode::TableFull,
        HbCode::GhostPid,
        HbCode::BindPlaneMismatch,
        HbCode::ConservationBroken,
        HbCode::NotReproducible,
        HbCode::IdempotenceBroken,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < HbCode::ALL.len() {
        if let Some(c) = HbCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F440 段闭集内"
}

// ── 槽位状态机（Free→Bound→Retired 单向） ──────────────────────────
/// 句柄槽三态（冻结 v1，只许单向推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 空闲：已注册未绑覆写。
    Free,
    /// 已绑：覆写已落到该句柄上。
    Bound,
    /// 退役：覆写已解绑/句柄注销，终态。
    Retired,
}

/// 单个句柄槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandleSlot {
    /// 句柄值（0=空槽哨兵）。
    pub handle: u32,
    /// 持有者进程号。
    pub owner: u32,
    /// 槽状态。
    pub state: SlotState,
    /// 绑定的覆写 id（未绑为 0）。
    pub bound_ovr: u32,
    /// 绑定序号（单调递增派生，用于双跑对拍）。
    pub bind_seq: u32,
}

impl HandleSlot {
    /// 空哨兵。
    pub const fn empty() -> HandleSlot {
        HandleSlot { handle: 0, owner: 0, state: SlotState::Free, bound_ovr: 0, bind_seq: 0 }
    }
    /// 空槽判定。
    pub fn is_empty(&self) -> bool {
        self.handle == 0
    }
}

// ── 位面：槽位"已绑覆写"标记 ───────────────────────────────────────
/// 绑定位面：12 槽用 u16 恰好装下。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BindPlane(pub u16);

impl BindPlane {
    pub const fn new() -> BindPlane {
        BindPlane(0)
    }
    /// 置位。
    pub fn set(&mut self, slot: usize) {
        if slot < HANDLE_CAP {
            self.0 |= 1u16 << slot;
        }
    }
    /// 清位（解绑时）。
    pub fn clear(&mut self, slot: usize) {
        if slot < HANDLE_CAP {
            self.0 &= !(1u16 << slot);
        }
    }
    /// 取位。
    pub fn get(self, slot: usize) -> bool {
        if slot < HANDLE_CAP {
            (self.0 >> slot) & 1u16 == 1u16
        } else {
            false
        }
    }
    /// 计数。
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

// ── 句柄注册表 + 覆写绑定 ──────────────────────────────────────────
/// 句柄注册表（定容 12 槽数组 + 绑定位面 + 守恒计数）。
pub struct HandleTable {
    pub slots: [HandleSlot; HANDLE_CAP],
    pub plane: BindPlane,
    /// 已注册句柄总数。
    pub registered_total: u32,
    /// 已退役槽总数。
    pub retired_total: u32,
    /// 被拒操作总数。
    pub rejected_total: u32,
    /// 绑定序号累加器（单调）。
    pub bind_seq_acc: u32,
    /// 拒因账。
    pub reject_log: Vec<u32>,
}

impl HandleTable {
    pub fn new() -> HandleTable {
        HandleTable {
            slots: [HandleSlot::empty(); HANDLE_CAP],
            plane: BindPlane::new(),
            registered_total: 0,
            retired_total: 0,
            rejected_total: 0,
            bind_seq_acc: 0,
            reject_log: Vec::new(),
        }
    }

    /// 按句柄值线性扫槽（None=幽灵句柄）。
    fn find_slot(&self, handle: u32) -> Option<usize> {
        let mut i = 0;
        while i < HANDLE_CAP {
            if let Some(s) = self.slots.get(i) {
                if !s.is_empty() && s.handle == handle {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 首个空槽（None=表满）。
    fn free_slot(&self) -> Option<usize> {
        let mut i = 0;
        while i < HANDLE_CAP {
            if let Some(s) = self.slots.get(i) {
                if s.is_empty() {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 注册句柄：owner=持有进程。同 (handle,owner) 幂等重入返回成功。
    pub fn register(&mut self, handle: u32, owner: u32) -> Result<(), u32> {
        if handle == 0 || handle > HANDLE_ID_MAX {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(HbCode::HandleOutOfRange.code());
            return Err(HbCode::HandleOutOfRange.code());
        }
        if owner == 0 || owner > PID_MAX {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(HbCode::PidOutOfRange.code());
            return Err(HbCode::PidOutOfRange.code());
        }
        if let Some(slot) = self.find_slot(handle) {
            // 幂等：同 owner 重注册视为已注册；异 owner 视为跨进程抢注。
            let cur_owner = match self.slots.get(slot) {
                Some(s) => s.owner,
                None => 0,
            };
            if cur_owner == owner {
                return Ok(());
            }
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(HbCode::OwnerMismatch.code());
            return Err(HbCode::OwnerMismatch.code());
        }
        let slot = match self.free_slot() {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(HbCode::TableFull.code());
                return Err(HbCode::TableFull.code());
            }
        };
        if let Some(s) = self.slots.get_mut(slot) {
            *s = HandleSlot {
                handle,
                owner,
                state: SlotState::Free,
                bound_ovr: 0,
                bind_seq: 0,
            };
        }
        self.registered_total = self.registered_total.saturating_add(1);
        Ok(())
    }

    /// 创建覆写并绑定句柄：核心语义——必须 owner==请求进程。
    pub fn create_override(&mut self, pid: u32, handle: u32, ovr_id: u32) -> Result<(), u32> {
        if ovr_id == 0 || ovr_id > 255 {
            // 覆写 id 域冻结 1..=255（单字节打包，256 字面量须 u32 槽无碍）。
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(HbCode::HandleOutOfRange.code());
            return Err(HbCode::HandleOutOfRange.code());
        }
        let slot = match self.find_slot(handle) {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(HbCode::HandleNotFound.code());
                return Err(HbCode::HandleNotFound.code());
            }
        };
        // 块内取值（避免可变借用期间再动 self.other_field）。
        let (owner, state) = match self.slots.get(slot) {
            Some(s) => (s.owner, s.state),
            None => (0, SlotState::Retired),
        };
        if owner != pid {
            // 跨进程覆写：v1 一票否决。
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(HbCode::OwnerMismatch.code());
            return Err(HbCode::OwnerMismatch.code());
        }
        let new_state = match state {
            SlotState::Free => {
                self.bind_seq_acc = self.bind_seq_acc.saturating_add(1);
                SlotState::Bound
            }
            SlotState::Bound => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(HbCode::AlreadyBound.code());
                return Err(HbCode::AlreadyBound.code());
            }
            SlotState::Retired => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(HbCode::SlotRetired.code());
                return Err(HbCode::SlotRetired.code());
            }
        };
        let seq = self.bind_seq_acc;
        if let Some(s) = self.slots.get_mut(slot) {
            s.state = new_state;
            s.bound_ovr = ovr_id;
            s.bind_seq = seq;
        }
        self.plane.set(slot);
        Ok(())
    }

    /// 解绑（覆写撤销路径，状态回退到 Free——回退只许 Bound→Free）。
    pub fn unbind(&mut self, pid: u32, handle: u32) -> Result<(), u32> {
        let slot = match self.find_slot(handle) {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(HbCode::HandleNotFound.code());
                return Err(HbCode::HandleNotFound.code());
            }
        };
        let (owner, state) = match self.slots.get(slot) {
            Some(s) => (s.owner, s.state),
            None => (0, SlotState::Retired),
        };
        if owner != pid {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(HbCode::OwnerMismatch.code());
            return Err(HbCode::OwnerMismatch.code());
        }
        match state {
            SlotState::Bound => {
                if let Some(s) = self.slots.get_mut(slot) {
                    s.state = SlotState::Free;
                    s.bound_ovr = 0;
                }
                self.plane.clear(slot);
                Ok(())
            }
            SlotState::Free => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(HbCode::AlreadyBound.code());
                Err(HbCode::AlreadyBound.code())
            }
            SlotState::Retired => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(HbCode::SlotRetired.code());
                Err(HbCode::SlotRetired.code())
            }
        }
    }

    /// 退役（句柄注销，终态，只进不出）。
    pub fn retire(&mut self, handle: u32) -> Result<(), u32> {
        let slot = match self.find_slot(handle) {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(HbCode::HandleNotFound.code());
                return Err(HbCode::HandleNotFound.code());
            }
        };
        let (state, bound) = match self.slots.get(slot) {
            Some(s) => (s.state, s.bound_ovr),
            None => (SlotState::Retired, 0),
        };
        match state {
            SlotState::Retired => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(HbCode::SlotRetired.code());
                Err(HbCode::SlotRetired.code())
            }
            _ => {
                if let Some(s) = self.slots.get_mut(slot) {
                    s.state = SlotState::Retired;
                    s.bound_ovr = 0;
                }
                if bound != 0 {
                    self.plane.clear(slot);
                }
                self.retired_total = self.retired_total.saturating_add(1);
                Ok(())
            }
        }
    }

    /// 只读：句柄当前状态与绑定覆写 id。
    pub fn peek(&self, handle: u32) -> Option<(SlotState, u32)> {
        let slot = self.find_slot(handle)?;
        match self.slots.get(slot) {
            Some(s) => Some((s.state, s.bound_ovr)),
            None => None,
        }
    }

    /// 绑定守恒：位面计数 = Bound 槽数。
    pub fn plane_audit(&self) -> Result<(), u32> {
        let mut bound_n = 0u32;
        let mut i = 0;
        while i < HANDLE_CAP {
            if let Some(s) = self.slots.get(i) {
                if !s.is_empty() && s.state == SlotState::Bound {
                    bound_n = bound_n.saturating_add(1);
                }
            }
            i += 1;
        }
        if self.plane.count() != bound_n {
            return Err(HbCode::BindPlaneMismatch.code());
        }
        Ok(())
    }

    /// 账目守恒：registered = 非空槽数（退役槽仍在册，仅状态终态；
    /// retired_total 是独立的终态累计账）。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut occupied = 0u32;
        let mut i = 0;
        while i < HANDLE_CAP {
            if let Some(s) = self.slots.get(i) {
                if !s.is_empty() {
                    occupied = occupied.saturating_add(1);
                }
            }
            i += 1;
        }
        if self.registered_total != occupied {
            return Err(HbCode::ConservationBroken.code());
        }
        Ok(())
    }
}

// ── 黄金全链：6 进程注册+绑定+解绑+退役 ────────────────────────────
/// 黄金全链：6 进程各持 1 句柄，覆写绑定→查询→解绑→退役全绿。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut t = HandleTable::new();
    let mut i = 0u32;
    while i < 6 {
        let pid = 10 + i;
        let handle = 100 + i;
        let ovr = 1 + i;
        t.register(handle, pid)?;
        t.create_override(pid, handle, ovr)?;
        // 绑定后 peek 应得 (Bound, ovr)。
        match t.peek(handle) {
            Some((SlotState::Bound, b)) if b == ovr => {}
            _ => return Err(HbCode::BindPlaneMismatch.code()),
        }
        t.unbind(pid, handle)?;
        t.retire(handle)?;
        i = i.saturating_add(1);
    }
    t.plane_audit()?;
    t.conserve()?;
    if t.retired_total != 6 {
        return Err(HbCode::ConservationBroken.code());
    }
    Ok((t.registered_total, t.retired_total))
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

/// 注入核：四类坏样本逐类 10/10 断言被显式拒绝。
///   槽 0：跨进程覆写（B 进程绑 A 的句柄）——判据核心
///   槽 1：幽灵句柄绑定
///   槽 2：重复绑定（已 Bound 再绑）
///   槽 3：退役再绑
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0：跨进程覆写——进程 20 绑进程 10 的句柄 100，必拒。
        let mut t0 = HandleTable::new();
        let _ = t0.register(100, 10);
        let hit0 = match t0.create_override(20, 100, 1) {
            Err(c) if c == HbCode::OwnerMismatch.code() => true,
            _ => false,
        };
        led.record(0, hit0);
        // 槽 1：幽灵句柄。
        let mut t1 = HandleTable::new();
        let _ = t1.register(200, 30);
        let hit1 = match t1.create_override(30, 999, 1) {
            Err(c) if c == HbCode::HandleNotFound.code() => true,
            _ => false,
        };
        led.record(1, hit1);
        // 槽 2：重复绑定。
        let mut t2 = HandleTable::new();
        let _ = t2.register(300, 40);
        let _ = t2.create_override(40, 300, 2);
        let hit2 = match t2.create_override(40, 300, 3) {
            Err(c) if c == HbCode::AlreadyBound.code() => true,
            _ => false,
        };
        led.record(2, hit2);
        // 槽 3：退役再绑。
        let mut t3 = HandleTable::new();
        let _ = t3.register(400, 50);
        let _ = t3.retire(400);
        let hit3 = match t3.create_override(50, 400, 5) {
            Err(c) if c == HbCode::SlotRetired.code() => true,
            _ => false,
        };
        led.record(3, hit3);
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ───────────────────────────────────────────────────────
/// 探针 A：正例绑定链全绿。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut t = HandleTable::new();
    t.register(700, 70)?;
    t.create_override(70, 700, 7)?;
    t.unbind(70, 700)?;
    Ok(())
}

/// 探针 B：跨进程必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut t = HandleTable::new();
    t.register(710, 71)?;
    t.create_override(72, 710, 1)
}

/// 探针 C：幽灵进程注册越界必拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut t = HandleTable::new();
    t.register(1, 0)
}

// ── 双跑确定性对拍 ─────────────────────────────────────────────────
/// 双跑：同黄金链两次逐字段相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(HbCode::NotReproducible.code());
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
    while i < HbCode::ALL.len() {
        if let Some(c) = HbCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 容量耗尽探测：连注册 13 句柄，第 13 条必返表满。
pub fn capacity_probe() -> Result<(u32, u32), u32> {
    let mut t = HandleTable::new();
    let mut filled = 0u32;
    let mut i = 1u32;
    while i <= 13u32 {
        match t.register(i * 10, 900) {
            Ok(()) => filled = filled.saturating_add(1),
            Err(c) if c == HbCode::TableFull.code() => {}
            Err(_) => return Err(HbCode::ConservationBroken.code()),
        }
        i = i.saturating_add(1);
    }
    if filled != HANDLE_CAP as u32 {
        return Err(HbCode::ConservationBroken.code());
    }
    Ok((filled, t.rejected_total))
}

/// 幂等注册探测：同 (handle,owner) 二次注册结果一致；异 owner 跨进程抢注必拒。
pub fn idempotence_probe() -> Result<(), u32> {
    let mut t = HandleTable::new();
    t.register(500, 60)?;
    t.register(500, 60)?; // 幂等重入
    match t.register(500, 61) {
        Err(c) if c == HbCode::OwnerMismatch.code() => {}
        _ => return Err(HbCode::IdempotenceBroken.code()),
    }
    Ok(())
}

// ── 判据收口 ───────────────────────────────────────────────────────
/// 判据产出（14 条）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链-绑定断言全过", g.is_ok(), format!("{:?}", g)));
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-跨进程注入10/10",
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
    // 绑定位面对账。
    let mut t = HandleTable::new();
    let _ = t.register(60, 6);
    let _ = t.create_override(6, 60, 6);
    let plane_ok = t.plane_audit().is_ok();
    v.push(("J2-绑定位面对账", plane_ok, String::from("plane.count=Bound数")));
    let conserve_ok = t.conserve().is_ok();
    v.push(("J2-账目守恒", conserve_ok, String::from("registered=占用+退役")));
    // 单调：退役只进不出（再退役被拒不改账）。
    let mut t2 = HandleTable::new();
    let _ = t2.register(61, 6);
    let _ = t2.retire(61);
    let rv = t2.retired_total;
    let _ = t2.retire(61);
    let mono = t2.retired_total == rv;
    v.push(("J3-退役单调-终态不重入", mono, format!("before={} after={}", rv, t2.retired_total)));
    // 幂等：注册重入一致。
    let idem = idempotence_probe();
    v.push(("J3-注册幂等-异主抢注必拒", idem.is_ok(), format!("{:?}", idem)));
    // 容量耗尽。
    let cap = capacity_probe();
    v.push(("J4-容量耗尽-12满13拒", cap.is_ok(), format!("{:?}", cap)));
    // 边界：handle 0 / 4097 拒，1 / 4096 收；pid 0 / 1000 拒。
    let mut t3 = HandleTable::new();
    let b0 = t3.register(0, 1).err() == Some(HbCode::HandleOutOfRange.code());
    let b1 = t3.register(4097, 1).err() == Some(HbCode::HandleOutOfRange.code());
    let b2 = t3.register(1, 1000).err() == Some(HbCode::PidOutOfRange.code());
    let b3 = t3.register(HANDLE_ID_MAX, PID_MAX).is_ok();
    v.push((
        "J4-域边界四类",
        b0 && b1 && b2 && b3,
        format!("0拒{} 4097拒{} pid0拒{} 4096收{}", b0, b1, b2, b3),
    ));
    // 探针三序。
    let pa = probe_seq_a().is_ok();
    let pb = probe_seq_b().is_err();
    let pc = probe_seq_c().is_err();
    v.push(("J4-探针三序", pa && pb && pc, format!("a={} b!={} c!={}", pa, pb, pc)));
    // 双跑一致。
    v.push(("J5-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // 段位守卫。
    v.push(("J5-段位守卫", segment_guard(), String::from("0x7F440 窗口闭集")));
    // 文案闭集。
    let mut det_ok = true;
    let mut k = 0;
    while k < HbCode::ALL.len() {
        if let Some(c) = HbCode::ALL.get(k) {
            if detail_of(c.code()) != c.detail() || c.detail().len() == 0 {
                det_ok = false;
            }
        }
        k += 1;
    }
    v.push(("J5-诊断文案闭集非空", det_ok, format!("codes={}", HbCode::ALL.len())));
    v.push(("J5-判据收口", v.len() + 1 == 14, format!("n={}", v.len() + 1)));
    v
}

/// 探针入口：F18482 进程内临时覆写创建语义自检（14 条）。
pub fn run_unx18482_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18482 ovr_create_bind");
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
        // 正常流：6 进程注册→绑定→查询→解绑→退役全绿。
        assert!(golden_run().is_ok(), "黄金全链应全绿");
    }

    #[test]
    fn bind_unbind_roundtrip() {
        // 往返：绑定→解绑→再绑定，往返后状态一致。
        let mut t = HandleTable::new();
        assert!(t.register(88, 8).is_ok());
        assert!(t.create_override(8, 88, 8).is_ok());
        assert!(t.unbind(8, 88).is_ok());
        assert!(t.create_override(8, 88, 9).is_ok());
        match t.peek(88) {
            Some((SlotState::Bound, b)) => assert!(b == 9, "往返后再绑应得新 ovr"),
            other => assert!(false, "往返后应为 Bound：{:?}", other),
        }
    }

    #[test]
    fn cross_process_injection_caught() {
        // 注入核心：跨进程覆写 10/10 全抓。
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.is_ok());
        if let Ok(l) = led {
            assert!(l.all_caught(), "四槽必须全抓");
            assert!(l.missed_total() == 0, "漏检必须为 0");
            assert!(l.tries[0] == JUDGE_ROUNDS && l.caught[0] == JUDGE_ROUNDS, "跨进程槽满轮全抓");
        }
    }

    #[test]
    fn owner_mismatch_exhaustive() {
        // 三个 owner 互绑交叉全拒（跨进程语义穷举 3 组）。
        let mut t = HandleTable::new();
        let _ = t.register(10, 1);
        let _ = t.register(20, 2);
        let _ = t.register(30, 3);
        assert!(t.create_override(2, 10, 1).err() == Some(HbCode::OwnerMismatch.code()));
        assert!(t.create_override(3, 10, 1).err() == Some(HbCode::OwnerMismatch.code()));
        assert!(t.create_override(1, 20, 2).err() == Some(HbCode::OwnerMismatch.code()));
        assert!(t.create_override(3, 20, 2).err() == Some(HbCode::OwnerMismatch.code()));
        assert!(t.create_override(1, 30, 3).err() == Some(HbCode::OwnerMismatch.code()));
        assert!(t.create_override(2, 30, 3).err() == Some(HbCode::OwnerMismatch.code()));
        // 正主绑定全过。
        assert!(t.create_override(1, 10, 1).is_ok());
        assert!(t.create_override(2, 20, 2).is_ok());
        assert!(t.create_override(3, 30, 3).is_ok());
    }

    #[test]
    fn capacity_exhaustion_boundary() {
        // 容量边界：12 满、13 拒。
        let cap = capacity_probe();
        assert!(cap.is_ok());
        if let Ok((filled, _rejected)) = cap {
            assert!(filled == HANDLE_CAP as u32);
        }
    }

    #[test]
    fn domain_bounds_four_classes() {
        // 四类边界：handle 0/1/4096/4097。
        let mut t = HandleTable::new();
        assert!(t.register(0, 1).err() == Some(HbCode::HandleOutOfRange.code()));
        assert!(t.register(1, 1).is_ok());
        assert!(t.register(HANDLE_ID_MAX, 1).is_ok());
        assert!(t.register(HANDLE_ID_MAX + 1, 1).err() == Some(HbCode::HandleOutOfRange.code()));
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
        let set = run_unx18482_checks();
        assert!(set.all_passed(), "14 条判据应全绿");
        assert!(set.len() == 14, "判据条数应冻结 14");
    }
}
