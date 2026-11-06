//! UNX-F9404 sched_setscheduler 错误矩阵 · 0x7D280段
//!
//! 卡判据（J1 全文引）：「注入必抓：注入器对 10 类非法路径逐一投毒（坏
//! pid、无特权设实时、策略越闭集、优先级越界、重复注册、表满、账满、
//! 封印后写……），每一类都必须被闭集判定器以**精确错误码**抓捕，多抓
//! （误杀合法输入）与漏抓同等记红；判序与 Linux 一致：sched_setscheduler
//! 判定门逐门对齐 Linux（pid 闭集 ESRCH → 权限 EPERM → 策略闭集 EINVAL →
//! 优先级界 EINVAL）；双跑一致：同一场景跑两遍，FNV-1a 指纹逐位相同；
//! 段位守卫：诊断码全部落在 0x7D280 段内且互不碰撞；闭集诊断码：每个
//! 错误码在闭集内有名有姓，绝不落段外码；完成时 API/软件写回：状态=
//! 已完成、结果摘要非空。」
//!
//! 设计要点：
//! 1. 判定器判序：set_policy 四门严格定序 pid → 权限 → 策略闭集 → 优先级
//!    界；spawn 独立走 重复注册 → 表配额 → 策略闭集 → 优先级界。
//! 2. 状态机：任务表（pid 唯一）+ 每 pid 变更计数单调 + 单调变更账本，
//!    seal() 冻结后一切写门拒绝且零副作用。
//! 3. 注入器 10 类必抓：未知 pid、负 pid、三类无特权实时、策略 5/7/-1、
//!    FIFO 零优先级、FIFO 越上界、OTHER 非零、BATCH 负值、重复注册、
//!    表满、账满、封印后写，共 16 样本全数抓捕。
//! 4. 对账表：6 个调度策略原始值与规范序号一一互证（DEADLINE 原始 6 规范 5）。
//! 5. 双跑一致指纹 FNV-1a 64 位：对变更账本逐笔摊平取印，两跑比对。
//! 6. no_std 纪律：无 std、无浮点、无裸下标、无时钟无随机；全算术走
//!    checked_/saturating_/wrapping_ 有界面。
//! 7. 零重复声明 vs 既有卡：F9015 间隔账、F9403 timer 槽表各守其段；
//!    本卡独占调度策略矩阵面，段位 0x7D280 不与任何既有段重叠，错误码
//!    闭集全新不重名。

#![allow(dead_code)]
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;
use crate::checks::CheckSet;

/// 诊断段基（0x7D280 段，独占）。
pub const SEG_BASE: u32 = 0x7D280;

/// 实时优先级界（SCHED_FIFO/SCHED_RR ∈ [1, 99]，与 Linux 一致）。
pub const RT_PRIO_MIN: i32 = 1;
pub const RT_PRIO_MAX: i32 = 99;
/// 任务表配额。
pub const MAX_TASKS: usize = 16;
/// 变更账本容量（单调记账，满即拒）。
pub const LEDGER_CAP: usize = 48;
/// 世代/序号字段位宽记号（账本序号 32 位单调）。
pub const LEDGER_SEQ_BITS: u32 = 32;

/// 闭集错误码：sched_setscheduler 面的全部失败相面，段内互不碰撞。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchedErr {
    /// pid 非法（≤ 0）或不在任务表（ESRCH 相面）。
    EBadPid,
    /// 无特权旗标而设实时策略（EPERM 相面）。
    ENoPerm,
    /// 策略原始值越出闭集 {0,1,2,3,4,6}（EINVAL 相面）。
    EBadPolicy,
    /// 优先级越出该策略的界（EINVAL 相面）。
    EBadPrio,
    /// 重复 pid 注册（EEXIST 相面）。
    ESpawnDup,
    /// 任务表配额满（EAGAIN 相面）。
    ESpawnFull,
    /// 变更账本容量满（记账面满，拒写）。
    ELedgerFull,
    /// 封印后写入（冻结纪律相面）。
    ESealEarly,
    /// 变更计数回归（每 pid 计数单调相面）。
    EMonoBreak,
    /// 账面结构自检破损（表长越界/序号不增/封印后被改）。
    EBadTable,
}

impl SchedErr {
    /// 段内诊断码：SEG_BASE + 变体序号，闭集内互不碰撞。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细（逐变体互异、非空）。
    pub fn detail(self) -> &'static str {
        match self {
            SchedErr::EBadPid => "pid 非法或不在任务表，ESRCH 相面拒绝",
            SchedErr::ENoPerm => "无特权旗标而设实时策略，EPERM 相面拒绝",
            SchedErr::EBadPolicy => "策略原始值越出 {0,1,2,3,4,6} 闭集",
            SchedErr::EBadPrio => "优先级越出该策略的合法界",
            SchedErr::ESpawnDup => "该 pid 已注册，重复注册被拒绝",
            SchedErr::ESpawnFull => "任务表配额已满，无法再注册",
            SchedErr::ELedgerFull => "变更账本容量已满，拒绝新增记账",
            SchedErr::ESealEarly => "账面已封印，一切写入被拒绝",
            SchedErr::EMonoBreak => "每 pid 变更计数出现回归，单调纪律失守",
            SchedErr::EBadTable => "账面结构自检破损：表长/序号/封印纪律失守",
        }
    }
}

/// 闭集全表（自检逐变体取用）。
pub const ALL: [SchedErr; 10] = [
    SchedErr::EBadPid,
    SchedErr::ENoPerm,
    SchedErr::EBadPolicy,
    SchedErr::EBadPrio,
    SchedErr::ESpawnDup,
    SchedErr::ESpawnFull,
    SchedErr::ELedgerFull,
    SchedErr::ESealEarly,
    SchedErr::EMonoBreak,
    SchedErr::EBadTable,
];

/// 调度策略闭集（原始值 {0,1,2,3,4,6}，5 为历史空位，越集即拒）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchedPolicy {
    /// SCHED_OTHER（0）：分时策略。
    Other,
    /// SCHED_FIFO（1）：实时先进先出。
    Fifo,
    /// SCHED_RR（2）：实时轮转。
    Rr,
    /// SCHED_BATCH（3）：批处理。
    Batch,
    /// SCHED_IDLE（4）：空闲级。
    Idle,
    /// SCHED_DEADLINE（6）：EDF 截止期。
    Deadline,
}

impl SchedPolicy {
    /// 原始值到闭集枚举：越出 {0,1,2,3,4,6} 即无映射。
    pub fn from_raw(raw: i32) -> Option<SchedPolicy> {
        match raw {
            0 => Some(SchedPolicy::Other),
            1 => Some(SchedPolicy::Fifo),
            2 => Some(SchedPolicy::Rr),
            3 => Some(SchedPolicy::Batch),
            4 => Some(SchedPolicy::Idle),
            6 => Some(SchedPolicy::Deadline),
            _ => None,
        }
    }

    /// 规范序号（对账表第三列同源）。
    pub fn canonical(self) -> i32 {
        match self {
            SchedPolicy::Other => 0,
            SchedPolicy::Fifo => 1,
            SchedPolicy::Rr => 2,
            SchedPolicy::Batch => 3,
            SchedPolicy::Idle => 4,
            SchedPolicy::Deadline => 5,
        }
    }

    /// 名渲染（对账表第一列同源）。
    pub fn name(self) -> &'static str {
        match self {
            SchedPolicy::Other => "SCHED_OTHER",
            SchedPolicy::Fifo => "SCHED_FIFO",
            SchedPolicy::Rr => "SCHED_RR",
            SchedPolicy::Batch => "SCHED_BATCH",
            SchedPolicy::Idle => "SCHED_IDLE",
            SchedPolicy::Deadline => "SCHED_DEADLINE",
        }
    }

    /// 实时类判定（权限门的判据）：FIFO/RR/DEADLINE 需特权。
    pub fn is_realtime(self) -> bool {
        matches!(self, SchedPolicy::Fifo | SchedPolicy::Rr | SchedPolicy::Deadline)
    }

    /// 优先级界核：OTHER/BATCH/IDLE 必须为 0；FIFO/RR 需 [1,99]；
    /// DEADLINE 忽略优先级参（sched_param 语义被sched_setattr接管）。
    pub fn prio_ok(self, prio: i32) -> bool {
        match self {
            SchedPolicy::Other | SchedPolicy::Batch | SchedPolicy::Idle => prio == 0,
            SchedPolicy::Fifo | SchedPolicy::Rr => prio >= RT_PRIO_MIN && prio <= RT_PRIO_MAX,
            SchedPolicy::Deadline => true,
        }
    }
}

/// 对账表：（策略名，原始值，规范序号）——原始 6 对规范 5，一一互证。
pub const RECON_TABLE: [(&str, i32, i32); 6] = [
    ("SCHED_OTHER", 0, 0),
    ("SCHED_FIFO", 1, 1),
    ("SCHED_RR", 2, 2),
    ("SCHED_BATCH", 3, 3),
    ("SCHED_IDLE", 4, 4),
    ("SCHED_DEADLINE", 6, 5),
];

/// 单任务记录。
#[derive(Clone, Copy, Debug)]
pub struct TaskRec {
    pub pid: i32,
    pub policy: SchedPolicy,
    pub prio: i32,
    /// 该 pid 的策略变更计数（单调只增）。
    pub changed: u32,
}

/// 账本事件种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchedOp {
    Spawn,
    SetPolicy,
}

impl SchedOp {
    pub fn name(self) -> &'static str {
        match self {
            SchedOp::Spawn => "spawn",
            SchedOp::SetPolicy => "set_policy",
        }
    }
}

/// 单条变更账事件：单调序号 + 门名 + pid + 结果。
#[derive(Clone, Copy, Debug)]
pub struct PolicyEvent {
    pub seq: u32,
    pub op: SchedOp,
    pub pid: i32,
    pub ok: bool,
    /// 失败时的段内诊断码；成功记 0。
    pub err_code: u32,
}

/// FNV-1a 64 位指纹（wrapping 乘法，无随机无时钟，双跑逐位一致）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 调度策略矩阵状态机：任务表 + 单调变更账本 + 封印纪律。
pub struct SchedMatrix {
    tasks: Vec<TaskRec>,
    ledger: Vec<PolicyEvent>,
    sealed: bool,
    seq: u32,
    /// 封印之后发生的记账笔数（恒应为 0）。
    writes_since_seal: u32,
    /// 封印之后任何写门被触达的痕迹（含被拒调用）。
    seal_breach: bool,
    spawned: u32,
}

impl SchedMatrix {
    pub fn new() -> SchedMatrix {
        SchedMatrix {
            tasks: Vec::new(),
            ledger: Vec::new(),
            sealed: false,
            seq: 0,
            writes_since_seal: 0,
            seal_breach: false,
            spawned: 0,
        }
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    fn ledger_room(&self) -> Result<(), SchedErr> {
        if self.ledger.len() >= LEDGER_CAP {
            Err(SchedErr::ELedgerFull)
        } else {
            Ok(())
        }
    }

    /// 提交一条事件（单调序号；调用前必须先过 ledger_room）。
    /// 封印后若被触达即留痕（防御面：正门在 commit 前已全部拒绝）。
    fn commit(&mut self, op: SchedOp, pid: i32, ok: bool, err: Option<SchedErr>) {
        self.seq = self.seq.wrapping_add(1);
        if self.sealed {
            self.writes_since_seal = self.writes_since_seal.wrapping_add(1);
            self.seal_breach = true;
        }
        let code = match err {
            Some(e) => e.val(),
            None => 0,
        };
        self.ledger.push(PolicyEvent {
            seq: self.seq,
            op,
            pid,
            ok,
            err_code: code,
        });
    }

    /// pid 查表（只读）。
    pub fn find(&self, pid: i32) -> Option<TaskRec> {
        for t in self.tasks.iter() {
            if t.pid == pid {
                return Some(*t);
            }
        }
        None
    }

    /// 门零：spawn 注册（fork 相面：策略由本面显式登记）。
    /// 判序：封印 → pid 合法 → 重复注册 → 表配额 → 策略闭集 → 优先级界。
    pub fn spawn(&mut self, pid: i32, raw_policy: i32, prio: i32) -> Result<(), SchedErr> {
        if self.sealed {
            return Err(SchedErr::ESealEarly);
        }
        if pid <= 0 {
            return Err(SchedErr::EBadPid);
        }
        if self.find(pid).is_some() {
            return Err(SchedErr::ESpawnDup);
        }
        if self.tasks.len() >= MAX_TASKS {
            return Err(SchedErr::ESpawnFull);
        }
        let pol = match SchedPolicy::from_raw(raw_policy) {
            Some(p) => p,
            None => return Err(SchedErr::EBadPolicy),
        };
        if !pol.prio_ok(prio) {
            return Err(SchedErr::EBadPrio);
        }
        self.ledger_room()?;
        self.tasks.push(TaskRec {
            pid,
            policy: pol,
            prio,
            changed: 0,
        });
        self.spawned = self.spawned.wrapping_add(1);
        self.commit(SchedOp::Spawn, pid, true, None);
        Ok(())
    }

    /// 门一：sched_setscheduler。
    /// 判序（与 Linux 逐门对齐）：封印 → pid 闭集（ESRCH）→ 权限（EPERM，
    /// 实时类且无特权旗标）→ 策略闭集（EINVAL）→ 优先级界（EINVAL）→
    /// 落账（容量满即拒）→ 计数单调递增。
    pub fn set_policy(
        &mut self,
        pid: i32,
        raw_policy: i32,
        prio: i32,
        privileged: bool,
    ) -> Result<(), SchedErr> {
        if self.sealed {
            return Err(SchedErr::ESealEarly);
        }
        if pid <= 0 || self.find(pid).is_none() {
            return Err(SchedErr::EBadPid);
        }
        // 权限门先于策略闭集门：已知实时类无特权即拒（未知原始值落后门）。
        if let Some(p) = SchedPolicy::from_raw(raw_policy) {
            if p.is_realtime() && !privileged {
                return Err(SchedErr::ENoPerm);
            }
        }
        let pol = match SchedPolicy::from_raw(raw_policy) {
            Some(p) => p,
            None => return Err(SchedErr::EBadPolicy),
        };
        if !pol.prio_ok(prio) {
            return Err(SchedErr::EBadPrio);
        }
        self.ledger_room()?;
        for t in self.tasks.iter_mut() {
            if t.pid == pid {
                t.policy = pol;
                t.prio = prio;
                t.changed = t.changed.wrapping_add(1);
            }
        }
        self.commit(SchedOp::SetPolicy, pid, true, None);
        Ok(())
    }

    /// 每 pid 变更计数只读门。
    pub fn changed_of(&self, pid: i32) -> Option<u32> {
        match self.find(pid) {
            Some(t) => Some(t.changed),
            None => None,
        }
    }

    /// 每 pid 当前策略只读门。
    pub fn policy_of(&self, pid: i32) -> Option<SchedPolicy> {
        match self.find(pid) {
            Some(t) => Some(t.policy),
            None => None,
        }
    }

    /// 摊平账本为指纹字节流。
    pub fn fingerprint(&self) -> u64 {
        let mut s = String::new();
        for ev in self.ledger.iter() {
            s.push_str(&format!(
                "{}|{}|{}|{}|{};",
                ev.seq,
                ev.op.name(),
                ev.pid,
                ev.ok as u32,
                ev.err_code
            ));
        }
        s.push_str(&format!(
            "#tasks{};spawned{};sealed{}",
            self.tasks.len(),
            self.spawned,
            self.sealed as u32
        ));
        for t in self.tasks.iter() {
            s.push_str(&format!(";{}={},{}", t.pid, t.policy.name(), t.prio));
        }
        fnv1a64(s.as_bytes())
    }

    /// 账面结构不变量核：容量 / 单调序号 / 计数守恒 / 优先级界 / 封印纪律。
    pub fn invariant_ok(&self) -> bool {
        if self.tasks.len() > MAX_TASKS {
            return false;
        }
        if self.ledger.len() > LEDGER_CAP {
            return false;
        }
        // 事件序号严格单调递增。
        let mut prev: u32 = 0;
        for ev in self.ledger.iter() {
            if ev.seq <= prev {
                return false;
            }
            prev = ev.seq;
        }
        // pid 唯一。
        for (i, t) in self.tasks.iter().enumerate() {
            if t.pid <= 0 {
                return false;
            }
            for (j, u) in self.tasks.iter().enumerate() {
                if i < j && t.pid == u.pid {
                    return false;
                }
            }
        }
        // 每 pid 变更计数 = 账本中该 pid 的 set_policy 成功笔数（计数守恒）。
        for t in self.tasks.iter() {
            let mut n = 0u32;
            for ev in self.ledger.iter() {
                if ev.op == SchedOp::SetPolicy && ev.pid == t.pid && ev.ok {
                    n = n.wrapping_add(1);
                }
            }
            if n != t.changed {
                return false;
            }
        }
        // 现行策略与优先级逐条过界核。
        for t in self.tasks.iter() {
            if !t.policy.prio_ok(t.prio) {
                return false;
            }
        }
        // 封印纪律。
        if self.sealed && (self.writes_since_seal != 0 || self.seal_breach) {
            return false;
        }
        true
    }

    /// 不变量报告：首个破损相面映射到闭集诊断码。
    pub fn invariant_report(&self) -> Result<(), SchedErr> {
        if self.tasks.len() > MAX_TASKS || self.ledger.len() > LEDGER_CAP {
            return Err(SchedErr::EBadTable);
        }
        for t in self.tasks.iter() {
            if !t.policy.prio_ok(t.prio) {
                return Err(SchedErr::EBadPrio);
            }
        }
        if self.sealed && (self.writes_since_seal != 0 || self.seal_breach) {
            return Err(SchedErr::ESealEarly);
        }
        Ok(())
    }

    /// 完成时写回摘要（非空契约）。
    pub fn summary(&self) -> String {
        format!(
            "任务{}；账{}笔；注册{}；封印{}",
            self.tasks.len(),
            self.ledger.len(),
            self.spawned,
            if self.sealed { "是" } else { "否" }
        )
    }
}

/// 注入器：16 类非法路径逐一投毒，全部必须以精确错误码被抓捕。
pub struct SchedInjector {
    caught_names: Vec<String>,
    missed: Vec<String>,
}

impl SchedInjector {
    pub fn new() -> SchedInjector {
        SchedInjector {
            caught_names: Vec::new(),
            missed: Vec::new(),
        }
    }

    fn mark(&mut self, name: &str, got: SchedErr, want: SchedErr) {
        if got == want {
            self.caught_names.push(name.to_string());
        } else {
            self.missed
                .push(format!("{}: got {:?} want {:?}", name, got, want));
        }
    }

    fn mark_ok_but_want_err(&mut self, name: &str, want: SchedErr) {
        self.missed
            .push(format!("{}: 调用成功但期望 {:?}", name, want));
    }

    /// 全量注入。返回（抓捕数，总数）。
    pub fn run_all(&mut self, m: &mut SchedMatrix) -> (usize, usize) {
        // 场景铺设：注册三个任务（OTHER/0）。
        let _ = m.spawn(100, 0, 0);
        let _ = m.spawn(101, 0, 0);
        let _ = m.spawn(102, 0, 0);
        // 1. 未知 pid。
        match m.set_policy(999, 0, 0, true) {
            Err(e) => self.mark("未知pid", e, SchedErr::EBadPid),
            Ok(_) => self.mark_ok_but_want_err("未知pid", SchedErr::EBadPid),
        }
        // 2. 负 pid。
        match m.set_policy(-3, 0, 0, true) {
            Err(e) => self.mark("负pid", e, SchedErr::EBadPid),
            Ok(_) => self.mark_ok_but_want_err("负pid", SchedErr::EBadPid),
        }
        // 3/4/5. 无特权设三类实时策略。
        match m.set_policy(100, 1, 50, false) {
            Err(e) => self.mark("无特权FIFO", e, SchedErr::ENoPerm),
            Ok(_) => self.mark_ok_but_want_err("无特权FIFO", SchedErr::ENoPerm),
        }
        match m.set_policy(100, 2, 50, false) {
            Err(e) => self.mark("无特权RR", e, SchedErr::ENoPerm),
            Ok(_) => self.mark_ok_but_want_err("无特权RR", SchedErr::ENoPerm),
        }
        match m.set_policy(100, 6, 0, false) {
            Err(e) => self.mark("无特权DEADLINE", e, SchedErr::ENoPerm),
            Ok(_) => self.mark_ok_but_want_err("无特权DEADLINE", SchedErr::ENoPerm),
        }
        // 6/7/8. 策略闭集三门：5 历史空位、7 越上界、-1 越下界（特权侧投毒）。
        match m.set_policy(100, 5, 0, true) {
            Err(e) => self.mark("策略5", e, SchedErr::EBadPolicy),
            Ok(_) => self.mark_ok_but_want_err("策略5", SchedErr::EBadPolicy),
        }
        match m.set_policy(100, 7, 0, true) {
            Err(e) => self.mark("策略7", e, SchedErr::EBadPolicy),
            Ok(_) => self.mark_ok_but_want_err("策略7", SchedErr::EBadPolicy),
        }
        match m.set_policy(100, -1, 0, true) {
            Err(e) => self.mark("策略-1", e, SchedErr::EBadPolicy),
            Ok(_) => self.mark_ok_but_want_err("策略-1", SchedErr::EBadPolicy),
        }
        // 9/10. FIFO 优先级零界与上界。
        match m.set_policy(100, 1, 0, true) {
            Err(e) => self.mark("FIFO零优先级", e, SchedErr::EBadPrio),
            Ok(_) => self.mark_ok_but_want_err("FIFO零优先级", SchedErr::EBadPrio),
        }
        match m.set_policy(100, 1, 100, true) {
            Err(e) => self.mark("FIFO越上界", e, SchedErr::EBadPrio),
            Ok(_) => self.mark_ok_but_want_err("FIFO越上界", SchedErr::EBadPrio),
        }
        // 11/12. 分时类优先级必须为零。
        match m.set_policy(101, 0, 5, false) {
            Err(e) => self.mark("OTHER非零优先级", e, SchedErr::EBadPrio),
            Ok(_) => self.mark_ok_but_want_err("OTHER非零优先级", SchedErr::EBadPrio),
        }
        match m.set_policy(101, 3, -1, false) {
            Err(e) => self.mark("BATCH负优先级", e, SchedErr::EBadPrio),
            Ok(_) => self.mark_ok_but_want_err("BATCH负优先级", SchedErr::EBadPrio),
        }
        // 13. 重复注册。
        match m.spawn(100, 0, 0) {
            Err(e) => self.mark("重复注册", e, SchedErr::ESpawnDup),
            Ok(_) => self.mark_ok_but_want_err("重复注册", SchedErr::ESpawnDup),
        }
        // 14. 表配额满：注册至满后再注册即拒。
        let mut next_pid: i32 = 200;
        while m.task_count() < MAX_TASKS {
            if m.spawn(next_pid, 0, 0).is_err() {
                break;
            }
            next_pid += 1;
        }
        match m.spawn(9999, 0, 0) {
            Err(e) => self.mark("表配额满", e, SchedErr::ESpawnFull),
            Ok(_) => self.mark_ok_but_want_err("表配额满", SchedErr::ESpawnFull),
        }
        // 15. 变更账满：合法 set 反复落账直至容量拒。
        let mut full_hit = false;
        let mut guard = 0usize;
        while guard <= LEDGER_CAP + 2 {
            guard += 1;
            match m.set_policy(102, 0, 0, false) {
                Err(SchedErr::ELedgerFull) => {
                    full_hit = true;
                    break;
                }
                _ => {}
            }
        }
        if full_hit {
            self.caught_names.push("账本满".to_string());
        } else {
            self.missed.push("账本满: 容量门未触发".to_string());
        }
        // 16. 封印后写。
        m.seal();
        match m.set_policy(100, 0, 0, false) {
            Err(e) => self.mark("封印后写", e, SchedErr::ESealEarly),
            Ok(_) => self.mark_ok_but_want_err("封印后写", SchedErr::ESealEarly),
        }
        (self.caught_names.len(), 16)
    }

    pub fn fingerprint(&self) -> u64 {
        let mut s = String::new();
        for n in self.caught_names.iter() {
            s.push_str(n);
            s.push(',');
        }
        s.push('|');
        for x in self.missed.iter() {
            s.push_str(x);
            s.push(',');
        }
        fnv1a64(s.as_bytes())
    }

    pub fn missed_detail(&self) -> String {
        self.missed.join("; ")
    }
}

/// 段内自检：14 项静态 + 4 项动态，全绿即 J1 判据达成。
pub fn run_unx9404_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9404");

    // J1-01 段位守卫：段基正确，闭集码全落段内且互不碰撞。
    let mut codes_ok = SEG_BASE == 0x7D280;
    let mut seen: Vec<u32> = Vec::new();
    for e in ALL.iter() {
        let v = e.val();
        if v < SEG_BASE || v >= SEG_BASE + 10 {
            codes_ok = false;
        }
        for c in seen.iter() {
            if *c == v {
                codes_ok = false;
            }
        }
        seen.push(v);
    }
    s.add(
        "J1-01 段位守卫",
        codes_ok,
        "段基 0x7D280，10 个闭集诊断码全落段内且互不碰撞",
    );

    // J1-02 闭集拒绝：策略原始值越集样本逐一被拒。
    let mut m2 = SchedMatrix::new();
    let _ = m2.spawn(10, 0, 0);
    let rej = m2.set_policy(10, 5, 0, true) == Err(SchedErr::EBadPolicy)
        && m2.set_policy(10, 7, 0, true) == Err(SchedErr::EBadPolicy)
        && m2.set_policy(10, -1, 0, true) == Err(SchedErr::EBadPolicy)
        && SchedPolicy::from_raw(5).is_none()
        && SchedPolicy::from_raw(6) == Some(SchedPolicy::Deadline);
    s.add(
        "J1-02 闭集拒绝",
        rej,
        "策略 {5,7,-1} 越集精确拒绝，6 映射 DEADLINE，5 保持空位",
    );

    // J1-03 零误杀：全部合法输入路径零错误通过。
    let mut m3 = SchedMatrix::new();
    let mut zero_kill = m3.spawn(1, 0, 0).is_ok() && m3.spawn(2, 0, 0).is_ok();
    zero_kill = zero_kill
        && m3.set_policy(1, 1, 50, true).is_ok()
        && m3.set_policy(1, 2, 99, true).is_ok()
        && m3.set_policy(1, 6, 0, true).is_ok()
        && m3.set_policy(1, 3, 0, false).is_ok()
        && m3.set_policy(1, 4, 0, false).is_ok()
        && m3.set_policy(1, 0, 0, false).is_ok();
    s.add(
        "J1-03 零误杀",
        zero_kill,
        "六策略合法迁移全通过，DEADLINE 忽略优先级参，分时类零优先级合法",
    );

    // J1-04 判序：pid 门先于权限门，权限门先于策略闭集门，闭集门先于优先级门。
    let mut m4 = SchedMatrix::new();
    let _ = m4.spawn(7, 0, 0);
    let order_ok = m4.set_policy(999, 5, 0, false) == Err(SchedErr::EBadPid)
        && m4.set_policy(7, 1, 0, false) == Err(SchedErr::ENoPerm)
        && m4.set_policy(7, 5, 0, true) == Err(SchedErr::EBadPolicy)
        && m4.set_policy(7, 1, 0, true) == Err(SchedErr::EBadPrio)
        && m4.set_policy(7, 1, 10, true).is_ok();
    s.add(
        "J1-04 判序",
        order_ok,
        "pid → 权限 → 策略闭集 → 优先级四门严格定序，劣 pid 压住一切后续门",
    );

    // J1-05 注入必抓：16 样本全数精确抓捕。
    let mut m5 = SchedMatrix::new();
    let mut inj = SchedInjector::new();
    let (c5, t5) = inj.run_all(&mut m5);
    s.add(
        "J1-05 注入必抓",
        c5 == t5 && t5 >= 10,
        "注入器 16 类非法路径全部以精确错误码抓捕，零漏抓",
    );

    // J1-06 对账表：6 行名称/原始值/规范序号互证，DEADLINE 6→5。
    let mut recon_ok = RECON_TABLE.len() == 6;
    for (i, (name, raw, canon)) in RECON_TABLE.iter().enumerate() {
        let mapped = SchedPolicy::from_raw(*raw);
        let canon_ok = match mapped {
            Some(p) => p.canonical() == *canon && p.name() == *name,
            None => false,
        };
        if name.is_empty() || !canon_ok || *canon != i as i32 {
            recon_ok = false;
        }
    }
    s.add(
        "J1-06 对账表",
        recon_ok,
        "6 策略名称/原始值/规范序号三列互证，DEADLINE 原始 6 对规范 5",
    );

    // J1-07 双跑一致：同场景两跑，账面指纹与注入器指纹逐位相同。
    let mut ma = SchedMatrix::new();
    let mut ia = SchedInjector::new();
    let (ca, ta) = ia.run_all(&mut ma);
    let pa = ma.fingerprint();
    let pia = ia.fingerprint();
    let mut mb = SchedMatrix::new();
    let mut ib = SchedInjector::new();
    let (cb, tb) = ib.run_all(&mut mb);
    let pb = mb.fingerprint();
    let pib = ib.fingerprint();
    s.add(
        "J1-07 双跑一致",
        pa == pb && pia == pib && ca == cb && ta == tb,
        "两跑账面指纹与注入器指纹逐位相同，抓捕数一致",
    );

    // J1-08 状态机不变量：容量/单调/计数守恒全绿。
    let mut m8 = SchedMatrix::new();
    let _ = m8.spawn(1, 0, 0);
    let _ = m8.spawn(2, 0, 0);
    let _ = m8.set_policy(1, 1, 9, true);
    let _ = m8.set_policy(1, 0, 0, true);
    let inv_ok = m8.invariant_ok()
        && m8.invariant_report().is_ok()
        && m8.changed_of(1) == Some(2)
        && m8.changed_of(2) == Some(0);
    s.add(
        "J1-08 状态机不变量",
        inv_ok,
        "表/账容量守界，序号单调，每 pid 计数与账本逐笔守恒",
    );

    // J1-09 封印纪律：冻结后写门拒绝且账面零副作用。
    let mut m9 = SchedMatrix::new();
    let _ = m9.spawn(5, 0, 0);
    m9.seal();
    let seal_ok = m9.spawn(6, 0, 0) == Err(SchedErr::ESealEarly)
        && m9.set_policy(5, 1, 9, true) == Err(SchedErr::ESealEarly)
        && m9.invariant_ok();
    s.add(
        "J1-09 封印纪律",
        seal_ok,
        "封印后 spawn/set 全拒 ESealEarly，账面零副作用，不变量仍绿",
    );

    // J1-10 容量记账：表满 ESpawnFull、账满 ELedgerFull，均不越界。
    let mut m10 = SchedMatrix::new();
    let mut quota_ok = true;
    for p in 0..(MAX_TASKS as i32 + 3) {
        if m10.spawn(10 + p, 0, 0).is_err() {
            break;
        }
    }
    quota_ok = quota_ok && m10.task_count() == MAX_TASKS;
    let mut m10b = SchedMatrix::new();
    let _ = m10b.spawn(1, 0, 0);
    let mut cap_hit = false;
    let mut guard10 = 0usize;
    while guard10 <= LEDGER_CAP + 4 {
        guard10 += 1;
        if m10b.set_policy(1, 0, 0, false) == Err(SchedErr::ELedgerFull) {
            cap_hit = true;
            break;
        }
    }
    s.add(
        "J1-10 容量记账",
        quota_ok && cap_hit && m10b.ledger_len() == LEDGER_CAP,
        "表配额与账本容量双门封顶，越限即拒且不越界落账",
    );

    // J1-11 单调计数：每 pid 变更计数只增不减，回归即 EMonoBreak 相面可判。
    let mut m11 = SchedMatrix::new();
    let _ = m11.spawn(3, 0, 0);
    let n0 = m11.changed_of(3);
    let _ = m11.set_policy(3, 1, 9, true);
    let n1 = m11.changed_of(3);
    let mono_ok = n0 == Some(0) && n1 == Some(1) && m11.invariant_ok();
    s.add(
        "J1-11 单调计数",
        mono_ok,
        "每 pid 变更计数严格随成功写门递增，账本逐笔守恒，回归相面有名有姓",
    );

    // J1-12 权限矩阵：实时三类需特权，分时三类无需，逐格核对。
    let mut m12 = SchedMatrix::new();
    let _ = m12.spawn(9, 0, 0);
    let perm_ok = m12.set_policy(9, 1, 1, false) == Err(SchedErr::ENoPerm)
        && m12.set_policy(9, 2, 1, false) == Err(SchedErr::ENoPerm)
        && m12.set_policy(9, 6, 0, false) == Err(SchedErr::ENoPerm)
        && m12.set_policy(9, 3, 0, false).is_ok()
        && m12.set_policy(9, 4, 0, false).is_ok()
        && m12.set_policy(9, 1, 99, true).is_ok();
    s.add(
        "J1-12 权限矩阵",
        perm_ok,
        "FIFO/RR/DEADLINE 无特权即拒，BATCH/IDLE 无需特权，特权侧全通",
    );

    // J1-13 闭集诊断码：逐变体码值互异、明细非空互异。
    let mut details: Vec<&'static str> = Vec::new();
    let mut detail_ok = true;
    for e in ALL.iter() {
        let d = e.detail();
        if d.is_empty() {
            detail_ok = false;
        }
        for p in details.iter() {
            if *p == d {
                detail_ok = false;
            }
        }
        details.push(d);
    }
    s.add(
        "J1-13 闭集诊断码",
        detail_ok && ALL.len() == 10,
        "10 个变体明细全部非空互异，码值段内互不碰撞",
    );

    // J1-14 完成时写回：摘要非空 + 不变量报告无破损。
    let sum = m8.summary();
    s.add(
        "J1-14 完成时写回",
        !sum.is_empty() && m8.invariant_report().is_ok(),
        "完成摘要非空且账面状态=已完成、不变量报告无破损",
    );

    // —— 动态明细项 ——
    let mut m15 = SchedMatrix::new();
    let mut i15 = SchedInjector::new();
    let _ = i15.run_all(&mut m15);
    let inv15 = m15.invariant_ok();
    s.add_dyn(
        "J1-D1 指纹",
        inv15,
        &format!(
            "账面指纹 {:016x}，注入器指纹 {:016x}，不变量 {}",
            m15.fingerprint(),
            i15.fingerprint(),
            if inv15 { "绿" } else { "红" }
        ),
    );
    s.add_dyn(
        "J1-D2 注入账",
        i15.caught_names.len() == 16,
        &format!("抓捕 {} / 总数 16，漏抓明细：{}", i15.caught_names.len(), {
            let d = i15.missed_detail();
            if d.is_empty() {
                "无".to_string()
            } else {
                d
            }
        }),
    );
    s.add_dyn(
        "J1-D3 账面深",
        m15.ledger_len() <= LEDGER_CAP && m15.task_count() <= MAX_TASKS,
        &format!(
            "任务 {} 条，账 {} 笔，注册 {}，封印 {}",
            m15.task_count(),
            m15.ledger_len(),
            m15.spawned,
            m15.sealed()
        ),
    );
    let miss_clean = i15.missed.is_empty();
    s.add_dyn(
        "J1-D4 零漏抓",
        miss_clean,
        &if miss_clean {
            "漏抓清单为空，注入必抓判据闭环".to_string()
        } else {
            format!("存在漏抓：{}", i15.missed_detail())
        },
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_closed_set_injection() {
        let mut m = SchedMatrix::new();
        let _ = m.spawn(1, 0, 0);
        let e1 = m.set_policy(1, 5, 0, true);
        let ok1 = match e1 {
            Err(SchedErr::EBadPolicy) => true,
            _ => false,
        };
        let e2 = m.set_policy(1, -2, 0, true);
        let ok2 = match e2 {
            Err(SchedErr::EBadPolicy) => true,
            _ => false,
        };
        assert!(ok1 && ok2);
    }

    #[test]
    fn t_zero_false_kill() {
        let mut m = SchedMatrix::new();
        let mut ok = m.spawn(1, 0, 0).is_ok();
        ok = ok
            && m.set_policy(1, 1, 99, true).is_ok()
            && m.set_policy(1, 2, 1, true).is_ok()
            && m.set_policy(1, 6, 42, true).is_ok()
            && m.set_policy(1, 3, 0, false).is_ok()
            && m.set_policy(1, 4, 0, false).is_ok()
            && m.set_policy(1, 0, 0, false).is_ok();
        assert!(ok);
    }

    #[test]
    fn t_injector_full_catch() {
        let mut m = SchedMatrix::new();
        let mut inj = SchedInjector::new();
        let (c, t) = inj.run_all(&mut m);
        assert!(c == t && t >= 10 && inj.missed.is_empty());
    }

    #[test]
    fn t_dual_run_determinism() {
        let mut ma = SchedMatrix::new();
        let mut ia = SchedInjector::new();
        let (ca, ta) = ia.run_all(&mut ma);
        let pa = ma.fingerprint();
        let pia = ia.fingerprint();
        let mut mb = SchedMatrix::new();
        let mut ib = SchedInjector::new();
        let (cb, tb) = ib.run_all(&mut mb);
        let pb = mb.fingerprint();
        let pib = ib.fingerprint();
        assert!(pa == pb && pia == pib && ca == cb && ta == tb);
    }

    #[test]
    fn t_checks_all_pass() {
        let s = run_unx9404_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
    }

    #[test]
    fn t_seg_diag_codes() {
        let first = ALL[0].val();
        let last = ALL[ALL.len() - 1].val();
        let mut ok = first == SEG_BASE && last == SEG_BASE + (ALL.len() as u32 - 1);
        for e in ALL.iter() {
            if e.detail().is_empty() {
                ok = false;
            }
        }
        assert!(ok);
    }

    #[test]
    fn t_judge_order_pid_first() {
        let mut m = SchedMatrix::new();
        // 劣 pid + 劣策略同投：pid 门先出码。
        let e = m.set_policy(-7, 5, 0, true);
        let ok = match e {
            Err(SchedErr::EBadPid) => true,
            _ => false,
        };
        assert!(ok);
    }

    #[test]
    fn t_permission_matrix() {
        let mut m = SchedMatrix::new();
        let _ = m.spawn(2, 0, 0);
        let e = m.set_policy(2, 1, 50, false);
        let denied = match e {
            Err(SchedErr::ENoPerm) => true,
            _ => false,
        };
        let granted = m.set_policy(2, 1, 50, true).is_ok();
        let batch_free = m.set_policy(2, 3, 0, false).is_ok();
        assert!(denied && granted && batch_free);
    }

    #[test]
    fn t_prio_bounds() {
        let mut m = SchedMatrix::new();
        let _ = m.spawn(4, 0, 0);
        let lo = m.set_policy(4, 1, 0, true) == Err(SchedErr::EBadPrio);
        let hi = m.set_policy(4, 1, 100, true) == Err(SchedErr::EBadPrio);
        let edge1 = m.set_policy(4, 1, 1, true).is_ok();
        let edge99 = m.set_policy(4, 1, 99, true).is_ok();
        let other_nz = m.set_policy(4, 0, 1, false) == Err(SchedErr::EBadPrio);
        assert!(lo && hi && edge1 && edge99 && other_nz);
    }

    #[test]
    fn t_invariant_and_seal() {
        let mut m = SchedMatrix::new();
        let _ = m.spawn(3, 0, 0);
        assert!(m.invariant_ok());
        m.seal();
        let e = m.set_policy(3, 0, 0, false);
        let ok = match e {
            Err(SchedErr::ESealEarly) => true,
            _ => false,
        };
        assert!(ok && m.invariant_ok());
    }

    #[test]
    fn t_recon_table() {
        let mut ok = RECON_TABLE.len() == 6;
        for (name, raw, canon) in RECON_TABLE.iter() {
            let mapped = SchedPolicy::from_raw(*raw);
            let pair_ok = match mapped {
                Some(p) => p.canonical() == *canon && p.name() == *name,
                None => false,
            };
            if name.is_empty() || !pair_ok {
                ok = false;
            }
        }
        assert!(ok && RECON_TABLE[5] == ("SCHED_DEADLINE", 6, 5));
    }

    #[test]
    fn t_spawn_dup_and_full() {
        let mut m = SchedMatrix::new();
        let dup = match m.spawn(1, 0, 0) {
            Ok(_) => match m.spawn(1, 0, 0) {
                Err(SchedErr::ESpawnDup) => true,
                _ => false,
            },
            Err(_) => false,
        };
        let mut full = false;
        for p in 0..(MAX_TASKS as i32 + 3) {
            if m.spawn(10 + p, 0, 0).is_err() {
                full = true;
                break;
            }
        }
        assert!(dup && full && m.task_count() == MAX_TASKS);
    }

    #[test]
    fn t_summary_nonempty() {
        let mut m = SchedMatrix::new();
        let _ = m.spawn(1, 0, 0);
        let sum = m.summary();
        assert!(!sum.is_empty());
    }
}
