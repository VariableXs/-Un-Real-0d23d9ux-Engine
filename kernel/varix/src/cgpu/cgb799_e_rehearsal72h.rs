//! CGPU-F0799 · E 域 72h 收官彩排（目标 340 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥340 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0799，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：E 域 72h 收官彩排——把收官动作排进 3×24 小时刻度表：
//!   任务（RehearsalTask）入槽、槽位冲突检测、执行打卡、
//!   回滚演练、彩排结论（绿/黄/红）与演练账本守恒。
//!
//! 设计纪律：
//! - 刻度闭集：72 个小时槽（tick 0..=71），越界即口径非法；
//!   任务必须落在 D1（0..=23）/ D2（24..=47）/ D3（48..=71）三天内。
//! - 任务状态机：Planned→Started→Done / Blocked；Blocked 必须
//!   在 4 槽内转 Done 或升级为 Escalated（终态）。
//! - 冲突口径：同槽任务数 ≤ SLOTS_PER_TICK=3，第 4 个入槽拒绝。
//! - 彩排结论闭集三态：Green（全 Done）/ Yellow（存在 Blocked
//!   但无 Escalated）/ Red（存在 Escalated）。
//! - 演练账本定容：TASK_CAP=96，超容逐出最旧终态任务（无则拒绝），
//!   守恒恒等式：total_tasks == held + evicted。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
//!   诊断段 0x6F20 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x6F20，批内 CGPU-F0799 独占）。
pub const SEG_BASE: u16 = 0x6F20;

/// 任务编号重复。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 槽位冲突（同槽超 3 个）。
pub const E_SLOT: u16 = SEG_BASE + 2;
/// 状态机非法转移。
pub const E_STATE: u16 = SEG_BASE + 3;
/// 未知任务。
pub const E_NOTASK: u16 = SEG_BASE + 4;
/// 槽位刻度越界（不在 0..=71）。
pub const E_TICK: u16 = SEG_BASE + 5;
/// 任务账本已满且无可逐出终态任务。
pub const E_CAP: u16 = SEG_BASE + 6;
/// 守恒破坏。
pub const E_LEDGER: u16 = SEG_BASE + 7;

/// 总刻度（72 小时）。
pub const TOTAL_TICKS: u32 = 72;
/// 单槽任务上限。
pub const SLOTS_PER_TICK: usize = 3;
/// Blocked 处理时限（槽）。
pub const BLOCK_TTL: u32 = 4;
/// 任务账本容量。
pub const TASK_CAP: usize = 96;

/// 任务状态（闭集五态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    Planned,
    Started,
    Done,
    Blocked,
    Escalated,
}

impl TaskState {
    pub fn is_final(self) -> bool {
        matches!(self, TaskState::Done | TaskState::Escalated)
    }
    pub fn ord(self) -> u8 {
        match self {
            TaskState::Planned => 0,
            TaskState::Started => 1,
            TaskState::Done => 2,
            TaskState::Blocked => 3,
            TaskState::Escalated => 4,
        }
    }
}

/// 彩排任务。
#[derive(Clone, Debug)]
pub struct RehearsalTask {
    /// 编号。
    pub id: u32,
    /// 名称。
    pub name: String,
    /// 入槽刻度。
    pub tick: u32,
    /// 状态。
    pub state: TaskState,
    /// Blocked 起始刻度（非 Blocked 为 None）。
    pub blocked_at: Option<u32>,
    /// 状态变更次数。
    pub transitions: u32,
}

impl RehearsalTask {
    /// 天序：0/1/2（D1/D2/D3）。
    pub fn day(&self) -> u8 {
        (self.tick / 24) as u8
    }
}

/// 彩排刻度表。
#[derive(Clone, Debug)]
pub struct RehearsalBoard {
    tasks: Vec<RehearsalTask>,
    evicted: u64,
    total_tasks: u64,
}

impl RehearsalBoard {
    pub fn new() -> Self {
        RehearsalBoard {
            tasks: Vec::new(),
            evicted: 0,
            total_tasks: 0,
        }
    }
    /// 排任务：刻度校验 + 槽位冲突检测。
    pub fn plan(&mut self, id: u32, name: &str, tick: u32) -> Result<(), u16> {
        if name.is_empty() {
            return Err(E_STATE);
        }
        if tick >= TOTAL_TICKS {
            return Err(E_TICK);
        }
        if self.tasks.iter().any(|t| t.id == id) {
            return Err(E_DUP);
        }
        let occupied = self.tasks.iter().filter(|t| t.tick == tick).count();
        if occupied >= SLOTS_PER_TICK {
            return Err(E_SLOT);
        }
        if self.tasks.len() >= TASK_CAP {
            let p = self.tasks.iter().position(|t| t.state.is_final());
            match p {
                Some(idx) => {
                    self.tasks.remove(idx);
                    self.evicted += 1;
                }
                None => return Err(E_CAP),
            }
        }
        self.tasks.push(RehearsalTask {
            id,
            name: name.to_string(),
            tick,
            state: TaskState::Planned,
            blocked_at: None,
            transitions: 0,
        });
        self.total_tasks += 1;
        Ok(())
    }
    fn pos(&self, id: u32) -> Result<usize, u16> {
        self.tasks.iter().position(|t| t.id == id).ok_or(E_NOTASK)
    }
    /// 开工：Planned→Started（允许 Started↔Blocked 往返）。
    pub fn start(&mut self, id: u32, now: u32) -> Result<(), u16> {
        if now >= TOTAL_TICKS {
            return Err(E_TICK);
        }
        let p = self.pos(id)?;
        let t = &mut self.tasks[p];
        if t.state != TaskState::Planned {
            return Err(E_STATE);
        }
        if now < t.tick {
            // 不能提前于排期开工。
            return Err(E_TICK);
        }
        t.state = TaskState::Started;
        t.transitions += 1;
        Ok(())
    }
    /// 完成：Started→Done 或 Blocked→Done。
    pub fn finish(&mut self, id: u32, now: u32) -> Result<(), u16> {
        if now >= TOTAL_TICKS {
            return Err(E_TICK);
        }
        let p = self.pos(id)?;
        let t = &mut self.tasks[p];
        let ok = match t.state {
            TaskState::Started => true,
            TaskState::Blocked => {
                let b = t.blocked_at.ok_or(E_STATE)?;
                // Blocked 超 TTL 未处理 → 必须升级，不允许悄悄 Done。
                if now > b + BLOCK_TTL {
                    return Err(E_STATE);
                }
                true
            }
            _ => false,
        };
        if !ok {
            return Err(E_STATE);
        }
        t.state = TaskState::Done;
        t.blocked_at = None;
        t.transitions += 1;
        Ok(())
    }
    /// 阻塞：Started→Blocked。
    pub fn block(&mut self, id: u32, now: u32) -> Result<(), u16> {
        if now >= TOTAL_TICKS {
            return Err(E_TICK);
        }
        let p = self.pos(id)?;
        let t = &mut self.tasks[p];
        if t.state != TaskState::Started {
            return Err(E_STATE);
        }
        t.state = TaskState::Blocked;
        t.blocked_at = Some(now);
        t.transitions += 1;
        Ok(())
    }
    /// 升级：Blocked→Escalated。
    pub fn escalate(&mut self, id: u32) -> Result<(), u16> {
        let p = self.pos(id)?;
        let t = &mut self.tasks[p];
        if t.state != TaskState::Blocked {
            return Err(E_STATE);
        }
        t.state = TaskState::Escalated;
        t.transitions += 1;
        Ok(())
    }
    /// 巡检：超 TTL 的 Blocked 自动升级。
    pub fn patrol(&mut self, now: u32) -> u32 {
        let mut n = 0u32;
        for t in self.tasks.iter_mut() {
            if t.state == TaskState::Blocked {
                if let Some(b) = t.blocked_at {
                    if now > b + BLOCK_TTL {
                        t.state = TaskState::Escalated;
                        t.transitions += 1;
                        n += 1;
                    }
                }
            }
        }
        n
    }
    /// 彩排结论（闭集三态）：0=Green / 1=Yellow / 2=Red。
    pub fn verdict(&self) -> Result<u8, u16> {
        if self.tasks.is_empty() {
            return Err(E_NOTASK);
        }
        let mut red = false;
        let mut yellow = false;
        for t in &self.tasks {
            match t.state {
                TaskState::Escalated => red = true,
                TaskState::Blocked | TaskState::Started | TaskState::Planned => yellow = true,
                TaskState::Done => {}
            }
        }
        Ok(if red {
            2
        } else if yellow {
            1
        } else {
            0
        })
    }
    /// 守恒审计：total_tasks == held + evicted。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total_tasks != self.tasks.len() as u64 + self.evicted {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    pub fn held(&self) -> usize {
        self.tasks.len()
    }
}

/// 自例程：主路径（Green 彩排）+ Yellow/Red 路径 + 槽位冲突 + 错误路径 + 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut b = RehearsalBoard::new();
    // 主路径 1：D1 排 4 个任务全部按期完成 → Green。
    b.plan(1, "freeze_api", 2)?;
    b.plan(2, "run_fuzz", 4)?;
    b.plan(3, "check_perf", 6)?;
    b.plan(4, "close_defects", 8)?;
    b.audit()?;
    for id in 1..=4u32 {
        b.start(id, id as u32)?;
        b.finish(id, id as u32 + 1)?;
    }
    if b.verdict()? != 0 {
        return Err(E_STATE);
    }
    // 主路径 2：一个 Blocked 在 TTL 内转 Done → Green；一个保持 Blocked → Yellow。
    b.plan(5, "rehearse_rollback", 20)?;
    b.start(5, 21)?;
    b.block(5, 22)?;
    b.finish(5, 25)?; // 22+4=26 边界内
    b.plan(6, "sign_off", 30)?;
    b.start(6, 30)?;
    b.block(6, 31)?;
    if b.verdict()? != 1 {
        return Err(E_STATE);
    }
    // 主路径 3：Blocked 超 TTL 被巡检升级 → Red。
    let n = b.patrol(36); // 31+4=35 < 36
    if n != 1 {
        return Err(E_LEDGER);
    }
    if b.verdict()? != 2 {
        return Err(E_STATE);
    }
    // Blocked 超 TTL 直接 finish 拒绝。
    b.plan(7, "late_fix", 40)?;
    b.start(7, 41)?;
    b.block(7, 42)?;
    if b.finish(7, 50) != Err(E_STATE) {
        return Err(E_STATE);
    }
    b.escalate(7)?;
    // 槽位冲突：tick 50 已排 1 个（task7 escalated at tick40，无妨），再排满 3 个后第 4 个拒绝。
    b.plan(8, "s1", 50)?;
    b.plan(9, "s2", 50)?;
    // 现在 tick50 有 task7? 不——task7 tick=40。当前 tick50 只有 8,9 → 再排 10,11 后第 5 个拒绝。
    b.plan(10, "s3", 50)?;
    if b.plan(11, "s4", 50) != Err(E_SLOT) {
        return Err(E_SLOT);
    }
    // 错误路径 1：刻度越界。
    if b.plan(12, "oob", 72) != Err(E_TICK) {
        return Err(E_TICK);
    }
    if b.start(1, 72) != Err(E_TICK) || b.finish(1, 99) != Err(E_TICK) {
        return Err(E_TICK);
    }
    // 错误路径 2：提前开工。
    b.plan(13, "early", 60)?;
    if b.start(13, 59) != Err(E_TICK) {
        return Err(E_TICK);
    }
    // 错误路径 3：状态跳步。
    b.plan(14, "jump", 60)?;
    if b.finish(14, 60) != Err(E_STATE) || b.block(14, 60) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 错误路径 4：重复编号 / 未知编号 / 空名。
    if b.plan(1, "dup", 55) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if b.start(99, 1) != Err(E_NOTASK) {
        return Err(E_NOTASK);
    }
    if b.plan(15, "", 55) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 天序核对：task7 tick40 → D2（day=1）。
    if b.tasks_day_of(7)? != 1 {
        return Err(E_TICK);
    }
    // 定容逐出：100 个任务（前 96 全 Done，后 4 入账触发逐出）。
    let mut b2 = RehearsalBoard::new();
    for i in 0..96u32 {
        let tick = i % TOTAL_TICKS;
        b2.plan(i + 1, "t", tick)?;
        b2.start(i + 1, tick)?;
        b2.finish(i + 1, tick)?;
    }
    for i in 0..4u32 {
        let tick = (96 + i) % TOTAL_TICKS;
        b2.plan(97 + i, "extra", tick)?;
    }
    b2.audit()?;
    if b2.held() != TASK_CAP || b2.evicted != 4 || b2.total_tasks != 100 {
        return Err(E_LEDGER);
    }
    // 满且全在册 → 拒绝。
    let mut b3 = RehearsalBoard::new();
    for i in 0..96u32 {
        let tick = i % TOTAL_TICKS;
        b3.plan(i + 1, "s", tick)?;
    }
    if b3.plan(97, "no room", 0) != Err(E_CAP) {
        return Err(E_CAP);
    }
    Ok(0)
}

impl RehearsalBoard {
    /// 只读天序查询。
    pub fn tasks_day_of(&self, id: u32) -> Result<u8, u16> {
        self.tasks
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.day())
            .ok_or(E_NOTASK)
    }
}
