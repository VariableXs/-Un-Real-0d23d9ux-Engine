//! UNX-F9286 setpgid 坏 pid ESRCH 矩阵 · 0x7B500段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F9286 逐字）：
//!   UNX-F9286-J1 坏 pid 注入判据过，全 ESRCH 格过，跨会话 EPERM 格过，
//!   与 B18 对账格过。
//!
//! 设计要点：
//!   ① 域模型是进程组/会话层次的定容进程表：每槽记录 pid/pgid/sid/state
//!      （Idle/Fresh/Execed/Zombie）加父指针与会话首标记；pid 号 = 槽号+1，
//!      号段 1..=PID_MAX，未挂表与越上界都算「pid 不存在」，一律 ESRCH。
//!   ② 判定次序钉死为 ESRCH 最先（负/零/未挂/越上界），其次 EACCES
//!      （已 exec、已终止、会话首），再 EPERM（跨会话、非子目标），最后
//!      才轮到 pgid 合法性——两条错误同时成立时必须返回靠前的一档。
//!   ③ pgid 面是逐码对账的深度点：Linux 对「pgid 指向的组不存在/不在同
//!      会话」回 EPERM 而非 EINVAL；唯独 pgid<0 回 EINVAL。两种「无效」
//!      必须分码落账，不许合并。
//!   ④ 自组即首规则：setpgid(0,0) 中 pid=0 解析为调用者、pgid=0 解析为
//!      目标 pid，调用者自成组长；调用者自身缺位时零号 pid 归 ESRCH。
//!   ⑤ 注入器三类（坏 pid/跨会话/坏 pgid）各 10 轮全抓，注入次数 =
//!      检出次数 + 漏检次数，漏检恒为 0；账目只增不减。
//!   ⑥ 与 B18 对账册冻结对齐：错误码只允许 EPERM/ESRCH/EACCES/EINVAL
//!      闭集，正负值逐行可复算，判序链 ESRCH→EACCES→EPERM→EINVAL 单调。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9286 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7B500;

/// pid 号段下界（0 号保留：pid=0 语义解析为调用者自身）。
pub const PID_FIRST: i32 = 1;

/// pid 号段上界（含）；超过即「越上界」。
pub const PID_MAX: i32 = 4096;

/// 进程表定容槽数（pid 1..=PROC_SLOTS 可挂表）。
pub const PROC_SLOTS: usize = 64;

/// 会话表定容槽数。
pub const SESSION_SLOTS: usize = 8;

/// 单类注入复测轮数（判据要求 10/10 全抓）。
pub const INJECT_ROUNDS: u32 = 10;

/// 注入类别数（坏 pid / 跨会话 / 坏 pgid）。
pub const INJECT_CLASSES: usize = 3;

/// init 进程（会话 1 的会话首）。
pub const INIT_PID: i32 = 1;

/// shell 进程（作业子进程的父）。
pub const SHELL_PID: i32 = 2;

/// 作业子进程 A。
pub const JOB_A_PID: i32 = 3;

/// 守护进程（会话 2 的会话首）。
pub const DAEMON_PID: i32 = 6;

/// canonical 场景中由 daemon fork 出的会话 2 非首子进程 pid。
pub const DAEMON_CHILD_PID: i32 = 7;

/// Linux errno 正数码：无权限。
pub const EPERM: i32 = 1;
/// Linux errno 正数码：无此进程。
pub const ESRCH: i32 = 3;
/// Linux errno 正数码：资源暂时不可用（fork 表满占位）。
pub const EAGAIN: i32 = 11;
/// Linux errno 正数码：权限不允许。
pub const EACCES: i32 = 13;
/// Linux errno 正数码：无效参数（本卡仅 pgid<0 一面）。
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=0x0F 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SetpgidCode {
    /// 负 pid：pid < 0
    NegativePid,
    /// 零号 pid：pid=0 但调用者自身缺位，无法解析
    ZeroPid,
    /// 未挂表：号段内但表中无此进程（含表槽已释放）
    UnusedPid,
    /// 越上界：pid > PID_MAX
    BeyondPidLimit,
    /// 目标已 exec：execve 之后不得再迁组
    ExecedTarget,
    /// 目标是会话首：会话首不得迁组
    SessionLeader,
    /// 目标已终止：僵尸槽仍在但生命周期已关闭
    TerminatedTarget,
    /// 跨会话：目标与调用者不在同一会话
    CrossSession,
    /// 非子目标：既不是调用者自身也不是其子进程
    NotChildOfCaller,
    /// pgid 指向的组不存在（Linux 回 EPERM 而非 EINVAL）
    PgidNoGroup,
    /// pgid 指向的组在别的会话
    PgidCrossSession,
    /// pgid < 0：Linux setpgid 唯一 EINVAL 面
    NegativePgid,
    /// 注入账倒退或漏检
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl SetpgidCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        // 1 基编码：段基址本身不占码，码窗 SEG_BASE+1..=SEG_BASE+15。
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SetpgidCode::NegativePid => "负 pid 被拒：pid 小于 0 不指向任何进程；\
                请核对目标来源，setpgid 一律回 ESRCH",
            SetpgidCode::ZeroPid => "零号 pid 无法解析：pid=0 本应指调用者，但调用者\
                自身不在进程表中；请先确认调用上下文再发起迁组",
            SetpgidCode::UnusedPid => "未挂表 pid 被拒：号段内但进程表无此槽位活动；\
                请先 fork 取得真实 pid，setpgid 回 ESRCH",
            SetpgidCode::BeyondPidLimit => "pid 越上界被拒：号大于 PID_MAX；请按 \
                1..=PID_MAX 约束取值，越界一律 ESRCH",
            SetpgidCode::ExecedTarget => "已 exec 目标被拒：execve 之后不得再改进程组；\
                请在 fork 与 exec 之间的窗口完成 setpgid，违例回 EACCES",
            SetpgidCode::SessionLeader => "会话首目标被拒：会话首进程不得迁组；\
                请先确认目标不是 setsid 的产物，违例按本卡口径回 EACCES",
            SetpgidCode::TerminatedTarget => "已终止目标被拒：僵尸槽保留账目但生命周期\
                已关闭；请 wait 收尸后再评估，迁组回 EACCES",
            SetpgidCode::CrossSession => "跨会话迁组被拒：目标与调用者不在同一会话；\
                setpgid 不允许跨会话操作，违例回 EPERM",
            SetpgidCode::NotChildOfCaller => "非子目标被拒：setpgid 只允许改调用者自身\
                或其直接子进程；请核对 parent 指针，违例回 EPERM",
            SetpgidCode::PgidNoGroup => "pgid 无组被拒：pgid 既不等于目标 pid 也不指向\
                同会话存活进程；Linux 对此回 EPERM 而非 EINVAL，请勿改码",
            SetpgidCode::PgidCrossSession => "pgid 跨会话被拒：目标组首存在但属其他\
                会话；迁组只能在同会话内进行，违例回 EPERM",
            SetpgidCode::NegativePgid => "负 pgid 被拒：pgid 小于 0 是 Linux setpgid \
                唯一的 EINVAL 面；请传 0 或正号，负值一律 EINVAL",
            SetpgidCode::LedgerRegress => "注入账倒退被拒：注入=检出+漏检且漏检为 0；\
                请检查回滚路径是否误改累计值",
            SetpgidCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定注入序列",
            SetpgidCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [SetpgidCode; 15] = [
        SetpgidCode::NegativePid,
        SetpgidCode::ZeroPid,
        SetpgidCode::UnusedPid,
        SetpgidCode::BeyondPidLimit,
        SetpgidCode::ExecedTarget,
        SetpgidCode::SessionLeader,
        SetpgidCode::TerminatedTarget,
        SetpgidCode::CrossSession,
        SetpgidCode::NotChildOfCaller,
        SetpgidCode::PgidNoGroup,
        SetpgidCode::PgidCrossSession,
        SetpgidCode::NegativePgid,
        SetpgidCode::LedgerRegress,
        SetpgidCode::NotReproducible,
        SetpgidCode::SegmentViolation,
    ];
}

/// 系统调用失败结果：携带 Linux errno 正数码。
pub type SysResult<T> = Result<T, i32>;

// ── 进程生命周期状态 ──────────────────────────────────────────────────
/// 进程槽状态。Idle 表示从未分配或已释放；Fresh 是 fork 产物未 exec；
/// Execed 是已 execve；Zombie 已终止但槽位尚未收尸。迁移方向单调：
/// Idle→Fresh→Execed→Zombie→Idle，任何越级或倒退都被显性拒绝。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcState {
    /// 空槽
    Idle,
    /// fork 产物，未 exec
    Fresh,
    /// 已 execve
    Execed,
    /// 已终止，待收尸
    Zombie,
}

/// 单个进程槽：进程组/会话层次的最小账目单元。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProcSlot {
    /// 生命周期状态
    pub state: ProcState,
    /// 本进程 pid（恒等于槽号 + PID_FIRST）
    pub pid: i32,
    /// 当前进程组号
    pub pgid: i32,
    /// 所属会话号
    pub sid: i32,
    /// 父进程 pid（init 为 0）
    pub parent: i32,
    /// 是否会话首
    pub sess_leader: bool,
    /// 成功迁组次数（账目，只增）
    pub setpgid_ok: u32,
}

impl ProcSlot {
    const fn empty() -> ProcSlot {
        ProcSlot {
            state: ProcState::Idle,
            pid: 0,
            pgid: 0,
            sid: 0,
            parent: 0,
            sess_leader: false,
            setpgid_ok: 0,
        }
    }

    /// fork 产物初值：pgid/sid 从父进程继承（Linux fork 语义）。
    const fn spawn(pid: i32, pgid: i32, sid: i32, parent: i32) -> ProcSlot {
        ProcSlot {
            state: ProcState::Fresh,
            pid,
            pgid,
            sid,
            parent,
            sess_leader: false,
            setpgid_ok: 0,
        }
    }
}

/// 会话槽：记录会话号、会话首与成员计数（账目，饱和增减）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SessionSlot {
    /// 会话号（0 = 未用）
    pub sid: i32,
    /// 会话首 pid
    pub leader: i32,
    /// 成员计数
    pub members: u32,
}

impl SessionSlot {
    const fn vacant() -> SessionSlot {
        SessionSlot { sid: 0, leader: 0, members: 0 }
    }
}

// ── 进程表 ────────────────────────────────────────────────────────────
/// 定容进程表 + 会话表。fork/exec/setsid/setpgid 都在真实槽位上推进
/// 状态机，所有取值走 get/get_mut，禁止裸下标。
#[derive(Clone, Copy, Debug)]
pub struct ProcTable {
    slots: [ProcSlot; PROC_SLOTS],
    sessions: [SessionSlot; SESSION_SLOTS],
    /// 累计成功 fork 次数
    forks: u32,
    /// 累计收尸（释放槽位）次数
    reaps: u32,
    /// 累计成功 setpgid 次数
    setpgid_calls: u32,
}

impl ProcTable {
    /// 空表构造。
    pub const fn new() -> ProcTable {
        ProcTable {
            slots: [ProcSlot::empty(); PROC_SLOTS],
            sessions: [SessionSlot::vacant(); SESSION_SLOTS],
            forks: 0,
            reaps: 0,
            setpgid_calls: 0,
        }
    }

    /// 规范场景表：
    ///   会话 1：init(1, 会话首) ← shell(2) ← 作业子 3/4/5；
    ///   会话 2：daemon(6) 经 setsid 迁出自成会话首。
    /// pid 7..=64 留作注入与容量测试的空槽。
    pub fn canonical() -> ProcTable {
        let mut t = ProcTable::new();
        if let Some(s) = t.slots.get_mut(0) {
            *s = ProcSlot {
                state: ProcState::Fresh,
                pid: INIT_PID,
                pgid: INIT_PID,
                sid: INIT_PID,
                parent: 0,
                sess_leader: true,
                setpgid_ok: 0,
            };
        }
        if let Some(ss) = t.sessions.get_mut(0) {
            *ss = SessionSlot { sid: INIT_PID, leader: INIT_PID, members: 1 };
        }
        let _ = t.fork(INIT_PID); // pid 2：shell
        let _ = t.fork(SHELL_PID); // pid 3：作业 A
        let _ = t.fork(SHELL_PID); // pid 4：作业 B
        let _ = t.fork(SHELL_PID); // pid 5：作业 C
        let daemon = match t.fork(INIT_PID) {
            Ok(p) => p,
            Err(_) => 0,
        };
        let _ = t.setsid(daemon); // pid 6 → 会话 2 会话首
        t
    }

    /// pid → 槽号。负值/零/越上界/号段内未挂表全部返回 None，
    /// 这是 ESRCH 的唯一来源层。
    fn slot_index(&self, pid: i32) -> Option<usize> {
        if pid < PID_FIRST || pid > PID_MAX {
            return None;
        }
        let off = (pid - PID_FIRST) as usize;
        if off >= PROC_SLOTS {
            return None;
        }
        Some(off)
    }

    /// fork：父必须存活；子槽取首个 Idle，pgid/sid 继承父值。表满 EAGAIN。
    pub fn fork(&mut self, parent: i32) -> SysResult<i32> {
        let pidx = match self.slot_index(parent) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        let (pstate, ppgid, psid) = match self.slots.get(pidx) {
            Some(s) => (s.state, s.pgid, s.sid),
            None => return Err(ESRCH),
        };
        match pstate {
            ProcState::Fresh | ProcState::Execed => {}
            ProcState::Zombie => return Err(EINVAL),
            ProcState::Idle => return Err(ESRCH),
        }
        let mut pos: Option<usize> = None;
        let mut i = 0usize;
        while i < PROC_SLOTS {
            if let Some(s) = self.slots.get(i) {
                if s.state == ProcState::Idle {
                    pos = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let idx = match pos {
            Some(i) => i,
            None => return Err(EAGAIN),
        };
        let pid = PID_FIRST + idx as i32;
        if let Some(s) = self.slots.get_mut(idx) {
            *s = ProcSlot::spawn(pid, ppgid, psid, parent);
        }
        if let Some(ss) = self.session_slot_mut(psid) {
            ss.members = ss.members.saturating_add(1);
        }
        self.forks = self.forks.saturating_add(1);
        Ok(pid)
    }

    /// exec：仅 Fresh 允许；Idle 无此进程回 ESRCH，重复 exec 或 Zombie
    /// 属非法转换回 EINVAL。
    pub fn exec(&mut self, pid: i32) -> SysResult<()> {
        let idx = match self.slot_index(pid) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        let st = match self.slots.get(idx) {
            Some(s) => s.state,
            None => return Err(ESRCH),
        };
        match st {
            ProcState::Fresh => {}
            ProcState::Execed | ProcState::Zombie => return Err(EINVAL),
            ProcState::Idle => return Err(ESRCH),
        }
        if let Some(s) = self.slots.get_mut(idx) {
            s.state = ProcState::Execed;
        }
        Ok(())
    }

    /// 终止：Fresh/Execed → Zombie；Idle 不存在，Zombie 不可再终止。
    pub fn terminate(&mut self, pid: i32) -> SysResult<()> {
        let idx = match self.slot_index(pid) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        let st = match self.slots.get(idx) {
            Some(s) => s.state,
            None => return Err(ESRCH),
        };
        match st {
            ProcState::Fresh | ProcState::Execed => {}
            ProcState::Zombie => return Err(EINVAL),
            ProcState::Idle => return Err(ESRCH),
        }
        if let Some(s) = self.slots.get_mut(idx) {
            s.state = ProcState::Zombie;
        }
        Ok(())
    }

    /// 收尸：Zombie → Idle，归还槽位与会话成员账；非 Zombie 拒绝。
    /// 会话首收尸不解散会话记录（本卡只保证成员账不丢）。
    pub fn release(&mut self, pid: i32) -> SysResult<()> {
        let idx = match self.slot_index(pid) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        let (st, sid) = match self.slots.get(idx) {
            Some(s) => (s.state, s.sid),
            None => return Err(ESRCH),
        };
        match st {
            ProcState::Zombie => {}
            ProcState::Fresh | ProcState::Execed => return Err(EINVAL),
            ProcState::Idle => return Err(ESRCH),
        }
        if let Some(s) = self.slots.get_mut(idx) {
            *s = ProcSlot::empty();
        }
        if let Some(ss) = self.session_slot_mut(sid) {
            ss.members = ss.members.saturating_sub(1);
        }
        self.reaps = self.reaps.saturating_add(1);
        Ok(())
    }

    /// setsid：仅 Fresh 且非组首非会话首者可开会话（B18 选举语义）；
    /// 违例 EPERM，状态不符 EINVAL，会话表满 EAGAIN。
    pub fn setsid(&mut self, pid: i32) -> SysResult<i32> {
        let idx = match self.slot_index(pid) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        let (st, leader_flag, pgid, old_sid) = match self.slots.get(idx) {
            Some(s) => (s.state, s.sess_leader, s.pgid, s.sid),
            None => return Err(ESRCH),
        };
        if st != ProcState::Fresh {
            return Err(EINVAL);
        }
        if leader_flag || pgid == pid {
            return Err(EPERM);
        }
        let mut pos: Option<usize> = None;
        let mut i = 0usize;
        while i < SESSION_SLOTS {
            if let Some(ss) = self.sessions.get(i) {
                if ss.sid == 0 {
                    pos = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let si = match pos {
            Some(i) => i,
            None => return Err(EAGAIN),
        };
        if let Some(s) = self.slots.get_mut(idx) {
            s.sid = pid;
            s.pgid = pid;
            s.sess_leader = true;
        }
        if let Some(ss) = self.sessions.get_mut(si) {
            *ss = SessionSlot { sid: pid, leader: pid, members: 1 };
        }
        if let Some(ss) = self.session_slot_mut(old_sid) {
            ss.members = ss.members.saturating_sub(1);
        }
        Ok(pid)
    }

    /// 按会话号取可变会话槽。
    fn session_slot_mut(&mut self, sid: i32) -> Option<&mut SessionSlot> {
        if sid == 0 {
            return None;
        }
        // 先只读扫描定位槽位，再单独取可变借用（避免跨迭代双重可变借用）。
        let mut hit = None;
        let mut i = 0usize;
        while i < SESSION_SLOTS {
            if let Some(ss) = self.sessions.get(i) {
                if ss.sid == sid {
                    hit = Some(i);
                }
            }
            i += 1;
        }
        match hit {
            Some(idx) => self.sessions.get_mut(idx),
            None => None,
        }
    }

    /// setpgid 判定（带显式调用者）。判定次序与卡面一致：
    ///   ESRCH（pid 不存在：负/零/未挂/越上界）→
    ///   EACCES（已 exec / 已终止 / 会话首）→
    ///   EPERM（跨会话 → 非子目标）→
    ///   pgid 面（pgid<0 EINVAL；无效组/跨会话组 EPERM）。
    /// 成功时真实写回目标 pgid 并记成功账。
    pub fn judge_setpgid_for(&mut self, caller: i32, pid: i32, pgid_in: i32) -> SysResult<()> {
        // 0. 调用者必须在表中存活（pid=0 的语义解析依赖它）
        let cidx = match self.slot_index(caller) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        let caller_alive = match self.slots.get(cidx) {
            Some(s) => s.state != ProcState::Idle,
            None => false,
        };
        if !caller_alive {
            return Err(ESRCH);
        }

        // 1. 目标解析：pid=0 指调用者；解析失败即 ESRCH 最先
        let target = if pid == 0 { caller } else { pid };
        let tidx = match self.slot_index(target) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        let t_state = match self.slots.get(tidx) {
            Some(s) => s.state,
            None => return Err(ESRCH),
        };
        if t_state == ProcState::Idle {
            return Err(ESRCH);
        }

        // 2. EACCES：已 exec / 已终止 / 会话首，先于一切会话与 pgid 判定
        let t_leader = match self.slots.get(tidx) {
            Some(s) => s.sess_leader,
            None => true,
        };
        match t_state {
            ProcState::Execed | ProcState::Zombie => return Err(EACCES),
            ProcState::Idle | ProcState::Fresh => {}
        }
        if t_leader {
            return Err(EACCES);
        }

        // 3. EPERM：跨会话先于父子关系
        let (t_sid, t_parent) = match self.slots.get(tidx) {
            Some(s) => (s.sid, s.parent),
            None => return Err(ESRCH),
        };
        let c_sid = match self.slots.get(cidx) {
            Some(s) => s.sid,
            None => return Err(ESRCH),
        };
        if t_sid != c_sid {
            return Err(EPERM);
        }

        // 4. EPERM：只允许改自身或直接子进程
        if target != caller && t_parent != caller {
            return Err(EPERM);
        }

        // 5. pgid 解析与校验
        let pgid = if pgid_in == 0 { target } else { pgid_in };
        if pgid < 0 {
            return Err(EINVAL);
        }
        if pgid != target {
            let gidx = match self.slot_index(pgid) {
                Some(i) => i,
                None => return Err(EPERM),
            };
            let g_ok = match self.slots.get(gidx) {
                Some(s) => s.state != ProcState::Idle && s.sid == t_sid,
                None => false,
            };
            if !g_ok {
                return Err(EPERM);
            }
        }

        // 6. 成功：真实写回 pgid 并记两笔账
        if let Some(s) = self.slots.get_mut(tidx) {
            s.pgid = pgid;
            s.setpgid_ok = s.setpgid_ok.saturating_add(1);
        }
        self.setpgid_calls = self.setpgid_calls.saturating_add(1);
        Ok(())
    }

    /// 视图：进程当前组号。
    pub fn group_of(&self, pid: i32) -> Option<i32> {
        let idx = self.slot_index(pid)?;
        match self.slots.get(idx) {
            Some(s) if s.state != ProcState::Idle => Some(s.pgid),
            _ => None,
        }
    }

    /// 视图：进程当前会话号。
    pub fn session_of(&self, pid: i32) -> Option<i32> {
        let idx = self.slot_index(pid)?;
        match self.slots.get(idx) {
            Some(s) if s.state != ProcState::Idle => Some(s.sid),
            _ => None,
        }
    }

    /// 存活进程数（非 Idle 槽）。
    pub fn live_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < PROC_SLOTS {
            if let Some(s) = self.slots.get(i) {
                if s.state != ProcState::Idle {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 活跃会话数。
    pub fn session_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < SESSION_SLOTS {
            if let Some(ss) = self.sessions.get(i) {
                if ss.sid != 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 存活进程覆盖的去重进程组数（成对去重，定容内 O(n^2) 完成）。
    pub fn group_count(&self) -> usize {
        let mut seen: [i32; PROC_SLOTS] = [0; PROC_SLOTS];
        let mut n = 0usize;
        let mut i = 0usize;
        while i < PROC_SLOTS {
            let pgid = match self.slots.get(i) {
                Some(s) if s.state != ProcState::Idle => s.pgid,
                _ => {
                    i += 1;
                    continue;
                }
            };
            let mut dup = false;
            let mut j = 0usize;
            while j < n {
                if let Some(g) = seen.get(j) {
                    if *g == pgid {
                        dup = true;
                    }
                }
                j += 1;
            }
            if !dup {
                if let Some(g) = seen.get_mut(n) {
                    *g = pgid;
                }
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// fork 成功账。
    pub fn forks(&self) -> u32 {
        self.forks
    }

    /// 收尸账。
    pub fn reaps(&self) -> u32 {
        self.reaps
    }

    /// setpgid 成功账。
    pub fn setpgid_calls(&self) -> u32 {
        self.setpgid_calls
    }
}

// ── 调用者绑定判定器 ──────────────────────────────────────────────────
/// 绑定调用者的 setpgid 判定器：对外暴露卡面要求的
/// `judge_setpgid(pid, pgid) -> Result<(), i32>` 签名。
pub struct SetpgidCaller<'a> {
    caller: i32,
    table: &'a mut ProcTable,
}

impl<'a> SetpgidCaller<'a> {
    /// 以 caller 身份绑定进程表。
    pub fn new(table: &'a mut ProcTable, caller: i32) -> SetpgidCaller<'a> {
        SetpgidCaller { caller, table }
    }

    /// 卡面签名判定入口。
    pub fn judge_setpgid(&mut self, pid: i32, pgid: i32) -> Result<(), i32> {
        self.table.judge_setpgid_for(self.caller, pid, pgid)
    }
}

// ── 坏 pid 面分类 ─────────────────────────────────────────────────────
/// 注入的坏 pid 到诊断面的映射（仅对确认不存在的 pid 调用）。
pub fn bad_pid_face(pid: i32) -> SetpgidCode {
    if pid < 0 {
        SetpgidCode::NegativePid
    } else if pid == 0 {
        SetpgidCode::ZeroPid
    } else if pid > PID_MAX {
        SetpgidCode::BeyondPidLimit
    } else {
        SetpgidCode::UnusedPid
    }
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入次数 = 检出 + 漏检，漏检恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JudgeLedger {
    /// 注入总次数
    pub attempts: u32,
    /// 按期望码检出次数
    pub caught: u32,
    /// 漏检次数（必须为 0）
    pub missed: u32,
}

impl JudgeLedger {
    const fn new() -> JudgeLedger {
        JudgeLedger { attempts: 0, caught: 0, missed: 0 }
    }

    /// 记一次注入结果（饱和计数，只增不减）。
    pub fn record(&mut self, caught: bool) {
        self.attempts = self.attempts.saturating_add(1);
        if caught {
            self.caught = self.caught.saturating_add(1);
        } else {
            self.missed = self.missed.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.caught.saturating_add(self.missed) == self.attempts
    }

    /// 零漏检。
    pub fn clean(&self) -> bool {
        self.missed == 0
    }
}

// ── 固定 LCG（无随机；种子固定即序列确定）────────────────────────────
/// 线性同余发生器：确定性，同种子同序列。
pub fn lcg_next(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    *state
}

// ── 注入矩阵 ──────────────────────────────────────────────────────────
/// 坏 pid 注入探针（caller, pid, pgid）：负 / 零（调用者缺位）/ 未挂 / 越上界。
pub const BAD_PID_PROBES: [(i32, i32, i32); 4] = [
    (SHELL_PID, -1, 0),
    (0, 0, 0),
    (SHELL_PID, 9, 0),
    (SHELL_PID, PID_MAX + 1, 0),
];

/// 坏 pgid 注入探针：未挂组 / 跨会话组 / 负值 / 越上界号。
pub const PGID_PROBES: [i32; 4] = [9, DAEMON_PID, -3, PID_MAX + 1];

/// 坏 pgid 期望码：负值 EINVAL，其余无效组 EPERM（Linux 深度点）。
pub fn expected_pgid_code(pgid: i32) -> i32 {
    if pgid < 0 {
        EINVAL
    } else {
        EPERM
    }
}

/// 三类注入 × 10 轮：坏 pid 全 ESRCH、跨会话全 EPERM、坏 pgid 逐码全中。
/// 全部注入均被判定器拒绝，不改表，因此整轮确定性可复测。
pub fn run_inject_matrix() -> (JudgeLedger, [u32; INJECT_CLASSES]) {
    let mut table = ProcTable::canonical();
    // 为跨会话面在会话 2 中备一个非首子进程（pid 7）
    let daemon_child = match table.fork(DAEMON_PID) {
        Ok(p) => p,
        Err(_) => -1,
    };
    let mut ledger = JudgeLedger::new();
    let mut per = [0u32; INJECT_CLASSES];

    // 类 0：坏 pid × 10，LCG 选变体，期望码恒为 ESRCH
    let mut rng: u32 = 0x9286_0001;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let pick = (lcg_next(&mut rng) % BAD_PID_PROBES.len() as u32) as usize;
        let caught = match BAD_PID_PROBES.get(pick) {
            Some(&(c, p, g)) => table.judge_setpgid_for(c, p, g) == Err(ESRCH),
            None => false,
        };
        ledger.record(caught);
        if caught {
            if let Some(n) = per.get_mut(0) {
                *n = n.saturating_add(1);
            }
        }
        r += 1;
    }

    // 类 1：跨会话 × 10，双向交替（会话 1 调用者 ↔ 会话 2 目标）
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let caught = if r % 2 == 0 {
            table.judge_setpgid_for(SHELL_PID, daemon_child, 0) == Err(EPERM)
        } else {
            table.judge_setpgid_for(daemon_child, JOB_A_PID, 0) == Err(EPERM)
        };
        ledger.record(caught);
        if caught {
            if let Some(n) = per.get_mut(1) {
                *n = n.saturating_add(1);
            }
        }
        r += 1;
    }

    // 类 2：坏 pgid × 10，逐探针期望码必须精确命中
    let mut rng2: u32 = 0x9286_0002;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let pick = (lcg_next(&mut rng2) % PGID_PROBES.len() as u32) as usize;
        let caught = match PGID_PROBES.get(pick) {
            Some(&g) => {
                let want = expected_pgid_code(g);
                table.judge_setpgid_for(SHELL_PID, JOB_A_PID, g) == Err(want)
            }
            None => false,
        };
        ledger.record(caught);
        if caught {
            if let Some(n) = per.get_mut(2) {
                *n = n.saturating_add(1);
            }
        }
        r += 1;
    }

    (ledger, per)
}

// ── Linux 逐码对照与 B18 对账册 ───────────────────────────────────────
/// (场景, 本卡正码, Linux 负值, 对账说明)。负值必须恒等于 -正码。
pub const B18_LEDGER: &[(&str, i32, i32, &str)] = &[
    ("setpgid 坏 pid", ESRCH, -ESRCH,
     "B18 边界等价类：目标不存在归 ESRCH（负/零/未挂/越上界并入）"),
    ("setpgid 已 exec", EACCES, -EACCES,
     "B18 判序链：exec 窗口关闭后迁组被 EACCES 拦截"),
    ("setpgid 会话首", EACCES, -EACCES,
     "本卡口径：会话首迁组并入 EACCES（卡面判据原文）"),
    ("setpgid 跨会话", EPERM, -EPERM,
     "B18 判序链：会话违例归 EPERM"),
    ("setpgid 非子目标", EPERM, -EPERM,
     "B18 判序链：非自身非子进程归 EPERM"),
    ("setpgid 无效 pgid", EPERM, -EPERM,
     "Linux 对无效 pgid 回 EPERM 而非 EINVAL，逐码不得漂移"),
    ("setpgid 负 pgid", EINVAL, -EINVAL,
     "Linux setpgid 唯一 EINVAL 面：pgid<0"),
];

/// 表内码判定：只允许对账册中的正值。
pub fn b18_agrees(code: i32) -> bool {
    if code <= 0 {
        return false;
    }
    let mut i = 0usize;
    while i < B18_LEDGER.len() {
        if let Some(row) = B18_LEDGER.get(i) {
            if row.1 == code {
                return row.2 == -code && !row.3.is_empty();
            }
        }
        i += 1;
    }
    false
}

/// 判序链位阶：ESRCH(0) < EACCES(1) < EPERM(2) < EINVAL(3)，表外码 9。
pub fn order_rank(code: i32) -> u32 {
    match code {
        ESRCH => 0,
        EACCES => 1,
        EPERM => 2,
        EINVAL => 3,
        _ => 9,
    }
}

/// 对账册逐行复算：名称/说明非空、正码为正、负值精确、码在闭集、位阶合法。
pub fn b18_ledger_ok() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < B18_LEDGER.len() {
        if let Some(&(name, code, neg, note)) = B18_LEDGER.get(i) {
            if name.is_empty() || note.is_empty() {
                ok = false;
            }
            if code <= 0 || neg != -code {
                ok = false;
            }
            if !b18_agrees(code) {
                ok = false;
            }
            if order_rank(code) > 3 {
                ok = false;
            }
        } else {
            ok = false;
        }
        i += 1;
    }
    // 判序链单调：位阶必须严格递增
    ok && order_rank(ESRCH) < order_rank(EACCES)
        && order_rank(EACCES) < order_rank(EPERM)
        && order_rank(EPERM) < order_rank(EINVAL)
}

/// errno 负值查表：命中返回负值，表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < B18_LEDGER.len() {
        if let Some(row) = B18_LEDGER.get(i) {
            if row.1 == code {
                return row.2;
            }
        }
        i += 1;
    }
    0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料探针（caller, pid, pgid）：覆盖正常流、四类坏 pid、pgid 面、
/// EACCES 面与跨会话面；按序执行，成功探针会真实改表（确定性依赖顺序）。
const CORPUS_PROBES: [(i32, i32, i32); 12] = [
    (SHELL_PID, JOB_A_PID, JOB_A_PID),  // Ok：自组成组
    (SHELL_PID, JOB_A_PID, SHELL_PID),  // Ok：加入父组
    (SHELL_PID, -1, 0),                 // ESRCH：负 pid
    (0, 0, 0),                          // ESRCH：零号调用者缺位
    (SHELL_PID, 9, 0),                  // ESRCH：未挂表
    (SHELL_PID, PID_MAX + 1, 0),        // ESRCH：越上界
    (SHELL_PID, JOB_A_PID, -3),         // EINVAL：负 pgid
    (SHELL_PID, JOB_A_PID, DAEMON_PID), // EPERM：pgid 跨会话组
    (SHELL_PID, JOB_A_PID, 9),          // EPERM：pgid 无组
    (SHELL_PID, INIT_PID, INIT_PID),    // EACCES：会话首目标
    (INIT_PID, 0, 0),                   // EACCES：自身是会话首
    (SHELL_PID, DAEMON_CHILD_PID, 0),   // EPERM：跨会话目标
];

/// 规范语料双跑指纹：逐探针结果编码为字节后 FNV-1a。
pub fn corpus_fingerprint() -> u32 {
    let mut table = ProcTable::canonical();
    let _ = table.fork(DAEMON_PID); // pid 7：会话 2 非首子进程
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < CORPUS_PROBES.len() {
        let res = match CORPUS_PROBES.get(i) {
            Some(&(c, p, g)) => table.judge_setpgid_for(c, p, g),
            None => Ok(()),
        };
        match res {
            Ok(()) => bytes.push(0x7Fu8),
            Err(e) => bytes.push((e & 0x7F) as u8),
        }
        i += 1;
    }
    fnv1a(&bytes)
}

/// 注入矩阵指纹：账本与逐类检出数逐字节编码。
pub fn matrix_fingerprint(ledger: &JudgeLedger, per: &[u32; INJECT_CLASSES]) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push((ledger.attempts & 0xFF) as u8);
    bytes.push(((ledger.attempts >> 8) & 0xFF) as u8);
    bytes.push((ledger.caught & 0xFF) as u8);
    bytes.push((ledger.missed & 0xFF) as u8);
    let mut i = 0usize;
    while i < per.len() {
        if let Some(n) = per.get(i) {
            bytes.push((*n & 0xFF) as u8);
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9286 判据集（≥14 条静态；动态明细 ≤8 条）。
pub fn run_unx9286_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9286");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7B500 0x40 对齐且落在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：父进程把子进程自组成新组，pgid 真实写回
    let mut t1 = ProcTable::canonical();
    let c1 = match t1.fork(SHELL_PID) {
        Ok(p) => p,
        Err(_) => 0,
    };
    let self_group_ok = {
        let r = t1.judge_setpgid_for(SHELL_PID, c1, c1);
        r.is_ok() && t1.group_of(c1) == Some(c1) && t1.group_count() == 3
    };
    s.add("正常流：setpgid(子,子) 自组成新组且 pgid 写回", self_group_ok,
          "合法父子迁组必须生效；进程组数由 2 增至 3");

    // 3. 自组即首：setpgid(0,0) 经卡面签名判定器
    let mut t2 = ProcTable::canonical();
    let zero_zero_ok = {
        let mut sc = SetpgidCaller::new(&mut t2, SHELL_PID);
        sc.judge_setpgid(0, 0).is_ok()
    } && t2.group_of(SHELL_PID) == Some(SHELL_PID)
        && t2.setpgid_calls() == 1;
    s.add("自组即首：setpgid(0,0) 调用者自成组长", zero_zero_ok,
          "pid=0/pgid=0 双零解析为调用者自组；成功账必须记一笔");

    // 4. 正常流：子进程加入同会话既有组
    let mut t3 = ProcTable::canonical();
    let join_ok = {
        let r1 = t3.judge_setpgid_for(SHELL_PID, SHELL_PID, SHELL_PID);
        let r2 = t3.judge_setpgid_for(SHELL_PID, JOB_A_PID, SHELL_PID);
        r1.is_ok() && r2.is_ok() && t3.group_of(JOB_A_PID) == Some(SHELL_PID)
    };
    s.add("正常流：子进程加入同会话既有进程组", join_ok,
          "pgid 指向同会话存活组长时必须生效，进程组数不变");

    // 5. 坏 pid 四类全 ESRCH 且逐类诊断面正确
    let mut t4 = ProcTable::canonical();
    let bad_pid_ok = t4.judge_setpgid_for(SHELL_PID, -1, 0) == Err(ESRCH)
        && t4.judge_setpgid_for(0, 0, 0) == Err(ESRCH)
        && t4.judge_setpgid_for(SHELL_PID, 9, 0) == Err(ESRCH)
        && t4.judge_setpgid_for(SHELL_PID, 65, 0) == Err(ESRCH)
        && t4.judge_setpgid_for(SHELL_PID, PID_MAX + 1, 0) == Err(ESRCH)
        && bad_pid_face(-1) == SetpgidCode::NegativePid
        && bad_pid_face(0) == SetpgidCode::ZeroPid
        && bad_pid_face(9) == SetpgidCode::UnusedPid
        && bad_pid_face(PID_MAX + 1) == SetpgidCode::BeyondPidLimit;
    s.add("坏 pid 四类：负/零/未挂表/越上界全 ESRCH 且面正确", bad_pid_ok,
          "pid 不存在一律最先回 ESRCH，四类诊断面逐一对位");

    // 6. 跨会话 EPERM（双向）
    let mut t5 = ProcTable::canonical();
    let dc5 = match t5.fork(DAEMON_PID) {
        Ok(p) => p,
        Err(_) => 0,
    };
    let cross_ok = t5.judge_setpgid_for(SHELL_PID, dc5, 0) == Err(EPERM)
        && t5.judge_setpgid_for(dc5, JOB_A_PID, 0) == Err(EPERM)
        && t5.session_of(dc5) == Some(DAEMON_PID)
        && t5.session_of(JOB_A_PID) == Some(INIT_PID);
    s.add("跨会话：双向互迁组均 EPERM 且会话号核实", cross_ok,
          "目标与调用者不同会话即拒；判定次序上先于父子关系检查");

    // 7. EACCES 面：已 exec 与会话首
    let mut t6 = ProcTable::canonical();
    let c6 = match t6.fork(SHELL_PID) {
        Ok(p) => p,
        Err(_) => 0,
    };
    let _ = t6.exec(c6);
    let eacces_ok = t6.judge_setpgid_for(SHELL_PID, c6, c6) == Err(EACCES)
        && t6.judge_setpgid_for(SHELL_PID, INIT_PID, INIT_PID) == Err(EACCES)
        && t6.judge_setpgid_for(INIT_PID, 0, 0) == Err(EACCES)
        && t6.judge_setpgid_for(SHELL_PID, DAEMON_PID, DAEMON_PID) == Err(EACCES);
    s.add("EACCES：已 exec 目标与会话首目标均被拒", eacces_ok,
          "exec 窗口关闭与会话首两条 EACCES 面先于跨会话 EPERM");

    // 8. 坏 pgid 面：无效组 EPERM、跨会话组 EPERM、负值 EINVAL
    let mut t7 = ProcTable::canonical();
    let pgid_ok = t7.judge_setpgid_for(SHELL_PID, JOB_A_PID, 9) == Err(EPERM)
        && t7.judge_setpgid_for(SHELL_PID, JOB_A_PID, 65) == Err(EPERM)
        && t7.judge_setpgid_for(SHELL_PID, JOB_A_PID, PID_MAX) == Err(EPERM)
        && t7.judge_setpgid_for(SHELL_PID, JOB_A_PID, DAEMON_PID) == Err(EPERM)
        && t7.judge_setpgid_for(SHELL_PID, JOB_A_PID, -1) == Err(EINVAL)
        && t7.judge_setpgid_for(SHELL_PID, JOB_A_PID, -3) == Err(EINVAL);
    s.add("坏 pgid：无效组 EPERM、负 pgid EINVAL（Linux 深度点）", pgid_ok,
          "Linux 对无效 pgid 回 EPERM 而非 EINVAL；负 pgid 是唯一 EINVAL 面");

    // 9. 注入矩阵：三类 × 10 轮全抓
    let (ledger, per) = run_inject_matrix();
    let matrix_ok = match per.get(0) {
        Some(n0) => match per.get(1) {
            Some(n1) => match per.get(2) {
                Some(n2) => *n0 == INJECT_ROUNDS && *n1 == INJECT_ROUNDS && *n2 == INJECT_ROUNDS,
                None => false,
            },
            None => false,
        },
        None => false,
    };
    s.add("注入矩阵：坏 pid/跨会话/坏 pgid 各 10/10 全抓", matrix_ok,
          "三类违约注入必须逐轮按期望码检出，不得漏检");

    // 10. 注入账守恒
    let ledger_ok = ledger.conserved() && ledger.clean()
        && ledger.attempts == INJECT_CLASSES as u32 * INJECT_ROUNDS;
    s.add("注入账守恒：注入=检出+漏检且漏检=0", ledger_ok,
          "30 次注入必须 30 次检出、0 漏检；账目只增不减");

    // 11. 判定次序：五组并发错误钉住先后
    let mut t8 = ProcTable::canonical();
    let c8 = match t8.fork(SHELL_PID) {
        Ok(p) => p,
        Err(_) => 0,
    };
    let _ = t8.exec(c8);
    let dc8 = match t8.fork(DAEMON_PID) {
        Ok(p) => p,
        Err(_) => 0,
    };
    let order_ok = t8.judge_setpgid_for(SHELL_PID, 9, -3) == Err(ESRCH)
        && t8.judge_setpgid_for(dc8, c8, c8) == Err(EACCES)
        && t8.judge_setpgid_for(SHELL_PID, DAEMON_PID, -3) == Err(EACCES)
        && t8.judge_setpgid_for(dc8, JOB_A_PID, -3) == Err(EPERM)
        && t8.judge_setpgid_for(SHELL_PID, JOB_A_PID, -3) == Err(EINVAL)
        && order_rank(ESRCH) < order_rank(EACCES)
        && order_rank(EACCES) < order_rank(EPERM)
        && order_rank(EPERM) < order_rank(EINVAL);
    s.add("判定次序：ESRCH→EACCES→EPERM(会话)→pgid 面逐级让位", order_ok,
          "两条错误同时成立时必须返回靠前一档；位阶链严格单调");

    // 12. 状态机单调：fork→exec→终止→收尸→复用
    let mut t9 = ProcTable::canonical();
    let c9 = match t9.fork(SHELL_PID) {
        Ok(p) => p,
        Err(_) => 0,
    };
    let exec_once = t9.exec(c9).is_ok();
    let exec_twice = t9.exec(c9) == Err(EINVAL);
    let execed_refused = t9.judge_setpgid_for(SHELL_PID, c9, c9) == Err(EACCES);
    let term_ok = t9.terminate(c9).is_ok();
    let zombie_refused = t9.judge_setpgid_for(SHELL_PID, c9, c9) == Err(EACCES);
    let free_ok = t9.release(c9).is_ok();
    let released_esrch = t9.judge_setpgid_for(SHELL_PID, c9, c9) == Err(ESRCH);
    let term_idle_esrch = t9.terminate(9) == Err(ESRCH);
    let sm_ok = exec_once && exec_twice && execed_refused && term_ok
        && zombie_refused && free_ok && released_esrch && term_idle_esrch
        && t9.reaps() == 1;
    s.add("状态机：exec/终止/收尸后判据单调演进且账目闭合", sm_ok,
          "Idle→Fresh→Execed→Zombie→Idle 单向迁移；越级与倒退显性拒绝");

    // 13. 容量耗尽：fork 至表满 EAGAIN、账目不丢、收尸后可复用
    let mut t10 = ProcTable::canonical();
    let live_before = t10.live_count();
    let mut made = 0u32;
    let mut cap_clean = true;
    while made < PROC_SLOTS as u32 {
        match t10.fork(SHELL_PID) {
            Ok(_) => made = made.saturating_add(1),
            Err(EAGAIN) => break,
            Err(_) => {
                cap_clean = false;
                break;
            }
        }
    }
    let full = t10.fork(SHELL_PID) == Err(EAGAIN);
    let acct_ok = t10.forks() == 5 + made && t10.live_count() == PROC_SLOTS;
    let reuse_ok = {
        let victim = PID_FIRST + live_before as i32; // pid 7：填表期第一个产物
        let killed = t10.terminate(victim).is_ok() && t10.release(victim).is_ok();
        let refilled = match t10.fork(SHELL_PID) {
            Ok(p) => p == victim,
            Err(_) => false,
        };
        killed && refilled
    };
    s.add("容量耗尽：表满 EAGAIN 且账目不丢、收尸后槽位可复用",
          cap_clean && made == (PROC_SLOTS - live_before) as u32 && full && acct_ok && reuse_ok,
          "64 槽全部占满才允许 EAGAIN；fork/收尸/复用账目逐笔闭合");

    // 14. 双跑一致：语料与矩阵指纹逐字段相等
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    let (l2, p2) = run_inject_matrix();
    let m1 = matrix_fingerprint(&ledger, &per);
    let m2 = matrix_fingerprint(&l2, &p2);
    s.add("双跑一致：语料指纹与矩阵指纹逐字段相等",
          f1 == f2 && m1 == m2 && ledger.attempts == l2.attempts && per == p2,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 15. B18 对账：对账册逐行可复算
    let recon_ok = b18_ledger_ok()
        && b18_agrees(ESRCH) && b18_agrees(EACCES)
        && b18_agrees(EPERM) && b18_agrees(EINVAL)
        && !b18_agrees(0) && !b18_agrees(EAGAIN) && !b18_agrees(9999);
    s.add("B18 对账：错误码册逐行复算且表外码拒绝", recon_ok,
          "对账册 7 行名称/说明/正负值/位阶全过；EAGAIN 与任意表外码不入册");

    // 16. Linux 逐码负值精确
    let neg_ok = linux_neg(ESRCH) == -ESRCH
        && linux_neg(EPERM) == -EPERM
        && linux_neg(EACCES) == -EACCES
        && linux_neg(EINVAL) == -EINVAL
        && linux_neg(11) == 0;
    s.add("Linux 逐码：errno 正负值精确、表外查表返回 0", neg_ok,
          "负值必须恒等于 -正码；表外码显性返回 0 供上层甄别");

    // 17. 文案闭集：15 个诊断码均有非空建议
    let text_ok = SetpgidCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带非空中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 18. 诊断码唯一（成对复算）
    let mut uniq = true;
    let mut i = 0usize;
    while i < SetpgidCode::ALL.len() {
        let mut j = i + 1;
        while j < SetpgidCode::ALL.len() {
            if let (Some(a), Some(b)) = (SetpgidCode::ALL.get(i), SetpgidCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：15 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）：账目、指纹、拓扑、段位四条
    let ledger_text = format!(
        "注入 {} / 检出 {} / 漏检 {}",
        ledger.attempts, ledger.caught, ledger.missed
    );
    s.add_dyn("注入账快照", ledger.conserved() && ledger.clean(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let topo = ProcTable::canonical();
    let topo_text = format!(
        "会话 {} / 存活进程 {} / 进程组 {}",
        topo.session_count(),
        topo.live_count(),
        topo.group_count()
    );
    let topo_ok = topo.session_count() == 2
        && topo.live_count() == 6
        && topo.group_count() == 2;
    s.add_dyn("拓扑快照", topo_ok, &topo_text);
    let seg_text = format!(
        "段窗 0x{:05X}..0x{:05X}（{} 码）",
        SEG_BASE,
        SEG_BASE + SetpgidCode::ALL.len() as u32,
        SetpgidCode::ALL.len()
    );
    s.add_dyn("段位快照", seg_ok, &seg_text);

    // 收口：判据条数达到声明值
    let n_before = s.len();
    s.add("收口：判据条数达到声明值", n_before >= 14,
          "静态判据条数必须达到声明下限；不足即骨架未完成");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_flows_new_group_and_join() {
        let mut t = ProcTable::canonical();
        let c = t.fork(SHELL_PID).unwrap_or(0);
        assert_eq!(c, 7);
        assert!(t.judge_setpgid_for(SHELL_PID, c, c).is_ok());
        assert_eq!(t.group_of(c), Some(c));
        assert_eq!(t.group_count(), 3);
        assert!(t.judge_setpgid_for(SHELL_PID, JOB_A_PID, c).is_ok());
        assert_eq!(t.group_of(JOB_A_PID), Some(c));
    }

    #[test]
    fn zero_zero_self_leader() {
        let mut t = ProcTable::canonical();
        {
            let mut sc = SetpgidCaller::new(&mut t, SHELL_PID);
            assert!(sc.judge_setpgid(0, 0).is_ok());
        }
        assert_eq!(t.group_of(SHELL_PID), Some(SHELL_PID));
        assert_eq!(t.setpgid_calls(), 1);
    }

    #[test]
    fn bad_pid_four_classes_esrch() {
        let mut t = ProcTable::canonical();
        assert_eq!(t.judge_setpgid_for(SHELL_PID, -1, 0), Err(ESRCH));
        assert_eq!(t.judge_setpgid_for(0, 0, 0), Err(ESRCH));
        assert_eq!(t.judge_setpgid_for(SHELL_PID, 9, 0), Err(ESRCH));
        assert_eq!(t.judge_setpgid_for(SHELL_PID, 65, 0), Err(ESRCH));
        assert_eq!(t.judge_setpgid_for(SHELL_PID, PID_MAX + 1, 0), Err(ESRCH));
        assert_eq!(bad_pid_face(-1), SetpgidCode::NegativePid);
        assert_eq!(bad_pid_face(0), SetpgidCode::ZeroPid);
        assert_eq!(bad_pid_face(9), SetpgidCode::UnusedPid);
        assert_eq!(bad_pid_face(PID_MAX + 1), SetpgidCode::BeyondPidLimit);
    }

    #[test]
    fn cross_session_eperm_both_directions() {
        let mut t = ProcTable::canonical();
        let dc = t.fork(DAEMON_PID).unwrap_or(0);
        assert_eq!(t.session_of(dc), Some(DAEMON_PID));
        assert_eq!(t.judge_setpgid_for(SHELL_PID, dc, 0), Err(EPERM));
        assert_eq!(t.judge_setpgid_for(dc, JOB_A_PID, 0), Err(EPERM));
        // 会话内但非子进程：目标不是调用者的直接子进程
        assert_eq!(t.judge_setpgid_for(JOB_A_PID, SHELL_PID, 0), Err(EPERM));
    }

    #[test]
    fn execed_and_session_leader_eacces() {
        let mut t = ProcTable::canonical();
        let c = t.fork(SHELL_PID).unwrap_or(0);
        assert!(t.exec(c).is_ok());
        assert_eq!(t.judge_setpgid_for(SHELL_PID, c, c), Err(EACCES));
        assert_eq!(t.judge_setpgid_for(SHELL_PID, INIT_PID, INIT_PID), Err(EACCES));
        assert_eq!(t.judge_setpgid_for(INIT_PID, 0, 0), Err(EACCES));
    }

    #[test]
    fn bad_pgid_eperm_vs_einval() {
        let mut t = ProcTable::canonical();
        // Linux 深度点：无效组回 EPERM 而非 EINVAL
        assert_eq!(t.judge_setpgid_for(SHELL_PID, JOB_A_PID, 9), Err(EPERM));
        assert_eq!(t.judge_setpgid_for(SHELL_PID, JOB_A_PID, DAEMON_PID), Err(EPERM));
        assert_eq!(t.judge_setpgid_for(SHELL_PID, JOB_A_PID, PID_MAX), Err(EPERM));
        // 负 pgid 是唯一 EINVAL 面
        assert_eq!(t.judge_setpgid_for(SHELL_PID, JOB_A_PID, -1), Err(EINVAL));
        assert_eq!(t.judge_setpgid_for(SHELL_PID, JOB_A_PID, -3), Err(EINVAL));
        // pgid=0 解析为目标 pid
        assert!(t.judge_setpgid_for(SHELL_PID, JOB_A_PID, 0).is_ok());
        assert_eq!(t.group_of(JOB_A_PID), Some(JOB_A_PID));
    }

    #[test]
    fn judge_order_pinned() {
        let mut t = ProcTable::canonical();
        let c = t.fork(SHELL_PID).unwrap_or(0);
        let _ = t.exec(c);
        let dc = t.fork(DAEMON_PID).unwrap_or(0);
        // ESRCH 最先：目标不存在时负 pgid 不得抢码
        assert_eq!(t.judge_setpgid_for(SHELL_PID, 9, -3), Err(ESRCH));
        // EACCES 先于跨会话 EPERM
        assert_eq!(t.judge_setpgid_for(dc, c, c), Err(EACCES));
        // 会话首 EACCES 先于 pgid EINVAL
        assert_eq!(t.judge_setpgid_for(SHELL_PID, DAEMON_PID, -3), Err(EACCES));
        // 跨会话 EPERM 先于 pgid EINVAL
        assert_eq!(t.judge_setpgid_for(dc, JOB_A_PID, -3), Err(EPERM));
        // 位阶链单调
        assert!(order_rank(ESRCH) < order_rank(EACCES));
        assert!(order_rank(EACCES) < order_rank(EPERM));
        assert!(order_rank(EPERM) < order_rank(EINVAL));
    }

    #[test]
    fn injector_three_classes_all_caught() {
        let (l, per) = run_inject_matrix();
        assert_eq!(l.attempts, INJECT_CLASSES as u32 * INJECT_ROUNDS);
        assert_eq!(l.missed, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn ledger_conserved() {
        let (l, _per) = run_inject_matrix();
        assert!(l.conserved());
        assert!(l.clean());
        assert_eq!(l.caught, l.attempts);
    }

    #[test]
    fn capacity_exhaustion_and_reuse() {
        let mut t = ProcTable::canonical();
        let live_before = t.live_count();
        let mut made = 0u32;
        let mut clean = true;
        while made < PROC_SLOTS as u32 {
            match t.fork(SHELL_PID) {
                Ok(_) => made += 1,
                Err(EAGAIN) => break,
                Err(_) => {
                    clean = false;
                    break;
                }
            }
        }
        assert!(clean);
        assert_eq!(made, (PROC_SLOTS - live_before) as u32);
        assert_eq!(t.live_count(), PROC_SLOTS);
        assert_eq!(t.forks(), 5 + made);
        assert_eq!(t.fork(SHELL_PID), Err(EAGAIN));
        // 收尸后槽位复用：pid 重新可得且账目只增
        let victim = PID_FIRST + live_before as i32;
        assert!(t.terminate(victim).is_ok());
        assert!(t.release(victim).is_ok());
        assert_eq!(t.reaps(), 1);
        assert_eq!(t.fork(SHELL_PID), Ok(victim));
    }

    #[test]
    fn state_machine_monotonic() {
        let mut t = ProcTable::canonical();
        let c = t.fork(SHELL_PID).unwrap_or(0);
        assert!(t.exec(c).is_ok());
        assert_eq!(t.exec(c), Err(EINVAL));
        assert_eq!(t.judge_setpgid_for(SHELL_PID, c, c), Err(EACCES));
        assert!(t.terminate(c).is_ok());
        assert_eq!(t.terminate(c), Err(EINVAL));
        assert_eq!(t.judge_setpgid_for(SHELL_PID, c, c), Err(EACCES));
        assert!(t.release(c).is_ok());
        assert_eq!(t.judge_setpgid_for(SHELL_PID, c, c), Err(ESRCH));
        assert_eq!(t.terminate(9), Err(ESRCH));
        assert_eq!(t.reaps(), 1);
    }

    #[test]
    fn deterministic_double_run_and_seg_guard() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        let (l1, p1) = run_inject_matrix();
        let (l2, p2) = run_inject_matrix();
        assert_eq!(matrix_fingerprint(&l1, &p1), matrix_fingerprint(&l2, &p2));
        assert_eq!(p1, p2);
        // 段位守卫与码值窗
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
        assert_eq!(SetpgidCode::ALL.len(), 15);
        assert_eq!(SetpgidCode::NegativePid.val(), SEG_BASE + 1);
        assert_eq!(SetpgidCode::SegmentViolation.val(), SEG_BASE + 15);
        for c in SetpgidCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn b18_ledger_reconciles() {
        assert!(b18_ledger_ok());
        assert!(b18_agrees(ESRCH));
        assert!(b18_agrees(EINVAL));
        assert!(!b18_agrees(0));
        assert!(!b18_agrees(EAGAIN));
        assert_eq!(linux_neg(ESRCH), -ESRCH);
        assert_eq!(linux_neg(11), 0);
        assert_eq!(B18_LEDGER.len(), 7);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9286_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
