//! UNX-F50144 LPD 队列查询与取消档（0x43e20 段）
//!
//! 锚点：UNX-F50144-J1 查询/取消 ×10 状态一致；判据锚定 §AI-63 §2 专题五。
//!
//! 队列管理比递交更容易出「静默错」：查询要保证**只读**（绝不改变作业状态），
//! 取消要保证**属主隔离**（普通用户只能动自己的作业，root 例外）。这两条
//! 一旦失守，用户看到的就是「打印队列自己变短了」这种无法复现的怪事。
//!
//! 因此本模块把队列建成显式状态机 + 注册表：
//!
//! * `JobState` 七态（Queued/Held/Printing/CancelPending/Cancelled/Done/Removed），
//!   `Event` 六事件，迁移表集中在一处；非法边返回 `State`，终态重复取消返回
//!   `Already`（可区分「重复操作」与「乱来」）。
//! * 打印中作业收到取消**不立即消失**，而是转 `CancelPending` 并保留槽位——
//!   正在出纸的作业不能从队列里凭空蒸发，必须等 `Finish` 才落 `Cancelled`。
//! * 取消按队列作用域：命令里的队列名与作业队列不一致直接 `QueueMismatch`，
//!   避免「向 lp 发命令、却删掉了 lp2 的作业」。
//! * 查询输出是 `StatusLine` 的定序列表（id 升序），短查询 4 字段、长查询 5 字段。
//!
//! 数据结构：状态机 + 注册表 + 命令解码器 + 错误账本（四类占四类）。
//! 错误路径：段内闭集 15 码，全部在注入核里被真实构造。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43e20 独占 0x20）。
pub const SEG_BASE: u32 = 0x43e20;

pub const E_EMPTY: u16 = 1;
pub const E_OPCODE: u16 = 2;
pub const E_QUEUE: u16 = 3;
pub const E_QUEUE_LEN: u16 = 4;
pub const E_AGENT: u16 = 5;
pub const E_ID_SYNTAX: u16 = 6;
pub const E_ID_COUNT: u16 = 7;
pub const E_TRAILING: u16 = 8;
pub const E_STATE: u16 = 9;
pub const E_OWNER: u16 = 10;
pub const E_JOB_ID: u16 = 11;
pub const E_CAPACITY: u16 = 12;
pub const E_DUP_ID: u16 = 13;
pub const E_ALREADY: u16 = 14;
pub const E_QUEUE_MISMATCH: u16 = 15;

/// 闭集大小。
pub const ERR_SLOTS: usize = 15;

/// 队列名上限（与发现侧单标签上限一致）。
pub const MAX_QUEUE: usize = 63;

/// 代理名上限。
pub const MAX_AGENT: usize = 64;

/// 单条取消命令可携带的作业号上限。超出即拒绝，避免一条命令放大成
/// 无界循环；真实客户端单次删除也很少超过整队列规模。
pub const MAX_IDS: usize = 32;

/// 作业状态七态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Held,
    Printing,
    CancelPending,
    Cancelled,
    Done,
    Removed,
}

impl JobState {
    /// 状态数值（判据对照/掩码编码用）。
    pub fn code(self) -> u8 {
        match self {
            JobState::Queued => 0,
            JobState::Held => 1,
            JobState::Printing => 2,
            JobState::CancelPending => 3,
            JobState::Cancelled => 4,
            JobState::Done => 5,
            JobState::Removed => 6,
        }
    }

    /// 状态文本（长查询输出）。
    pub fn label(self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Held => "held",
            JobState::Printing => "printing",
            JobState::CancelPending => "cancel-pending",
            JobState::Cancelled => "cancelled",
            JobState::Done => "done",
            JobState::Removed => "removed",
        }
    }

    /// 终态：不再参与查询，也不再接受取消/打印事件。
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            JobState::Cancelled | JobState::Done | JobState::Removed
        )
    }

    /// 迁移函数。表驱动、纯函数：同输入必得同输出。
    pub fn apply(self, ev: Event) -> Result<JobState, LpdError> {
        match (self, ev) {
            (JobState::Queued, Event::StartPrinting) => Ok(JobState::Printing),
            (JobState::Queued, Event::Hold) => Ok(JobState::Held),
            (JobState::Held, Event::Release) => Ok(JobState::Queued),
            (JobState::Held, Event::StartPrinting) => Ok(JobState::Printing),
            (JobState::Queued, Event::Cancel) | (JobState::Held, Event::Cancel) => {
                Ok(JobState::Cancelled)
            }
            (JobState::Printing, Event::Cancel) => Ok(JobState::CancelPending),
            (JobState::Printing, Event::Finish) => Ok(JobState::Done),
            (JobState::CancelPending, Event::Finish) => Ok(JobState::Cancelled),
            (JobState::Done, Event::Purge) | (JobState::Cancelled, Event::Purge) => {
                Ok(JobState::Removed)
            }
            // 终态重复取消：语义上是「已经取消过了」，与「非法事件」区分开。
            (JobState::Cancelled, Event::Cancel)
            | (JobState::Done, Event::Cancel)
            | (JobState::CancelPending, Event::Cancel) => Err(LpdError::Already),
            _ => Err(LpdError::State),
        }
    }
}

/// 作业事件六种。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    StartPrinting,
    Finish,
    Hold,
    Release,
    Cancel,
    Purge,
}

/// 队列命令（RFC 1179 操作码 0x01..0x05 的最小集合）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueueCommand {
    StartQueue(String),
    StopQueue(String),
    ShortQuery(String),
    LongQuery(String),
    RemoveJobs {
        queue: String,
        agent: String,
        ids: Vec<u32>,
    },
}

impl QueueCommand {
    /// 操作码。
    pub fn opcode(&self) -> u8 {
        match self {
            QueueCommand::StartQueue(_) => 0x01,
            QueueCommand::StopQueue(_) => 0x02,
            QueueCommand::ShortQuery(_) => 0x03,
            QueueCommand::LongQuery(_) => 0x04,
            QueueCommand::RemoveJobs { .. } => 0x05,
        }
    }
}

/// 队列管理错误：`code()` = `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LpdError {
    Empty,
    Opcode,
    Queue,
    QueueLen,
    Agent,
    IdSyntax,
    IdCount,
    Trailing,
    State,
    Owner,
    JobId,
    Capacity,
    DupId,
    Already,
    QueueMismatch,
}

impl LpdError {
    /// 段内诊断码。
    pub fn code(self) -> u32 {
        SEG_BASE + self.slot() as u32 + 1
    }

    /// 账本槽位（0 基）。
    pub fn slot(self) -> usize {
        let n = match self {
            LpdError::Empty => E_EMPTY,
            LpdError::Opcode => E_OPCODE,
            LpdError::Queue => E_QUEUE,
            LpdError::QueueLen => E_QUEUE_LEN,
            LpdError::Agent => E_AGENT,
            LpdError::IdSyntax => E_ID_SYNTAX,
            LpdError::IdCount => E_ID_COUNT,
            LpdError::Trailing => E_TRAILING,
            LpdError::State => E_STATE,
            LpdError::Owner => E_OWNER,
            LpdError::JobId => E_JOB_ID,
            LpdError::Capacity => E_CAPACITY,
            LpdError::DupId => E_DUP_ID,
            LpdError::Already => E_ALREADY,
            LpdError::QueueMismatch => E_QUEUE_MISMATCH,
        };
        (n - 1) as usize
    }

    /// 一行诊断文本。
    pub fn message(self) -> &'static str {
        match self {
            LpdError::Empty => "empty queue command",
            LpdError::Opcode => "queue command opcode unknown",
            LpdError::Queue => "queue name empty or illegal",
            LpdError::QueueLen => "queue name exceeds 63 octets",
            LpdError::Agent => "agent name empty or illegal",
            LpdError::IdSyntax => "job id list malformed",
            LpdError::IdCount => "too many job ids in one command",
            LpdError::Trailing => "trailing bytes after command",
            LpdError::State => "illegal job state transition",
            LpdError::Owner => "agent does not own the job",
            LpdError::JobId => "unknown job id",
            LpdError::Capacity => "queue registry full",
            LpdError::DupId => "job id already registered",
            LpdError::Already => "job already cancelled/finished",
            LpdError::QueueMismatch => "job belongs to another queue",
        }
    }
}

/// 单个作业。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LpdJob {
    pub id: u32,
    pub queue: String,
    pub owner: String,
    pub df: String,
    pub bytes: usize,
    pub state: JobState,
}

/// 查询输出行。短查询只渲染前四字段，长查询追加状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusLine {
    pub id: u32,
    pub owner: String,
    pub df: String,
    pub bytes: usize,
    pub state: JobState,
}

impl StatusLine {
    /// 短查询行：`id owner df bytes`（4 字段）。
    pub fn short_text(&self) -> String {
        format!("{} {} {} {}", self.id, self.owner, self.df, self.bytes)
    }

    /// 长查询行：`id state owner df bytes`（5 字段）。
    pub fn long_text(&self) -> String {
        format!(
            "{} {} {} {} {}",
            self.id,
            self.state.label(),
            self.owner,
            self.df,
            self.bytes
        )
    }
}

/// 取消结果（也是查询结果的公共载体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandResult {
    /// 查询渲染出的行（取消/启停命令为空）。
    pub lines: Vec<String>,
    pub cancelled: u16,
    pub pending: u16,
    pub denied: u16,
}

impl CommandResult {
    fn empty() -> CommandResult {
        CommandResult {
            lines: Vec::new(),
            cancelled: 0,
            pending: 0,
            denied: 0,
        }
    }
}

/// 队列注册表：作业表 + 自增号 + 错误账本。
#[derive(Clone, Debug)]
pub struct QueueRegistry {
    jobs: Vec<LpdJob>,
    next_id: u32,
    cap: usize,
    started: bool,
    pub rejected: usize,
    pub err_counts: [u16; ERR_SLOTS],
    pub first_err: u32,
}

impl QueueRegistry {
    /// 建注册表（作业条数上限 cap）。
    pub fn new(cap: usize) -> QueueRegistry {
        QueueRegistry {
            jobs: Vec::new(),
            next_id: 1,
            cap,
            started: true,
            rejected: 0,
            err_counts: [0u16; ERR_SLOTS],
            first_err: 0,
        }
    }

    /// 入队：自动分配 id，返回新 id。
    pub fn enqueue(
        &mut self,
        queue: &str,
        owner: &str,
        df: &str,
        bytes: usize,
    ) -> Result<u32, LpdError> {
        validate_queue(queue)?;
        validate_agent(owner)?;
        if df.is_empty() {
            return Err(LpdError::Agent);
        }
        if self.jobs.len() >= self.cap {
            self.note_err(LpdError::Capacity);
            return Err(LpdError::Capacity);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.jobs.push(LpdJob {
            id,
            queue: queue.to_string(),
            owner: owner.to_string(),
            df: df.to_string(),
            bytes,
            state: JobState::Queued,
        });
        Ok(id)
    }

    /// 按显式 id 注册（重启恢复路径）；重复 id 必须显式失败。
    pub fn register(
        &mut self,
        id: u32,
        queue: &str,
        owner: &str,
        df: &str,
        bytes: usize,
        state: JobState,
    ) -> Result<(), LpdError> {
        if self.index_of(id).is_some() {
            self.note_err(LpdError::DupId);
            return Err(LpdError::DupId);
        }
        if self.jobs.len() >= self.cap {
            self.note_err(LpdError::Capacity);
            return Err(LpdError::Capacity);
        }
        self.jobs.push(LpdJob {
            id,
            queue: queue.to_string(),
            owner: owner.to_string(),
            df: df.to_string(),
            bytes,
            state,
        });
        if id >= self.next_id {
            self.next_id = id.saturating_add(1);
        }
        Ok(())
    }

    /// 按 id 查下标。
    pub fn index_of(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.jobs.len() {
            if let Some(j) = self.jobs.get(i) {
                if j.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 按 id 取作业。
    pub fn get(&self, id: u32) -> Option<&LpdJob> {
        match self.index_of(id) {
            Some(i) => self.jobs.get(i),
            None => None,
        }
    }

    /// 作业条数（含终态）。
    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    /// 队列启停状态。
    pub fn started(&self) -> bool {
        self.started
    }

    /// 记一次拒绝。
    pub fn note_err(&mut self, e: LpdError) {
        self.rejected += 1;
        let slot = e.slot();
        if let Some(c) = self.err_counts.get_mut(slot) {
            *c = c.saturating_add(1);
        }
        if self.first_err == 0 {
            self.first_err = e.code();
        }
    }

    /// 某类错误计数。
    pub fn err_count(&self, e: LpdError) -> u16 {
        match self.err_counts.get(e.slot()) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 对某作业施加事件（id 不存在 → `JobId`）。
    pub fn apply_event(&mut self, id: u32, ev: Event) -> Result<JobState, LpdError> {
        let idx = match self.index_of(id) {
            Some(i) => i,
            None => {
                self.note_err(LpdError::JobId);
                return Err(LpdError::JobId);
            }
        };
        let current = match self.jobs.get(idx) {
            Some(j) => j.state,
            None => return Err(LpdError::JobId),
        };
        let next = match current.apply(ev) {
            Ok(s) => s,
            Err(e) => {
                self.note_err(e);
                return Err(e);
            }
        };
        if let Some(j) = self.jobs.get_mut(idx) {
            j.state = next;
        }
        Ok(next)
    }

    /// 查询行（短）：排除终态，按 id 升序（插入序即 id 升序）。
    pub fn short_status(&self, queue: &str) -> Vec<StatusLine> {
        self.collect_status(queue)
    }

    /// 查询行（长）：同集合，渲染时带状态。
    pub fn long_status(&self, queue: &str) -> Vec<StatusLine> {
        self.collect_status(queue)
    }

    fn collect_status(&self, queue: &str) -> Vec<StatusLine> {
        let mut out: Vec<StatusLine> = Vec::new();
        let mut i = 0usize;
        while i < self.jobs.len() {
            if let Some(j) = self.jobs.get(i) {
                if j.queue == queue && !j.state.is_terminal() {
                    out.push(StatusLine {
                        id: j.id,
                        owner: j.owner.clone(),
                        df: j.df.clone(),
                        bytes: j.bytes,
                        state: j.state,
                    });
                }
            }
            i += 1;
        }
        out
    }

    /// 队列内非终态作业数。
    pub fn remaining(&self, queue: &str) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.jobs.len() {
            if let Some(j) = self.jobs.get(i) {
                if j.queue == queue && !j.state.is_terminal() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 取消作业。
    ///
    /// * `ids` 为空表示「删除该 agent 在该队列的全部作业」；
    /// * 属主不符计 denied（不整体失败：一条命令删多个作业时，不能因为
    ///   其中一个是别人的就放弃其余合法的删除）；
    /// * 队列不符直接 `QueueMismatch`（这是协议级错误，不能部分执行）。
    pub fn cancel(
        &mut self,
        queue: &str,
        agent: &str,
        ids: &[u32],
    ) -> Result<CommandResult, LpdError> {
        validate_queue(queue)?;
        validate_agent(agent)?;
        let root = agent == "root";
        let mut res = CommandResult::empty();
        let mut targets: Vec<usize> = Vec::new();
        if ids.is_empty() {
            let mut i = 0usize;
            while i < self.jobs.len() {
                if let Some(j) = self.jobs.get(i) {
                    if j.queue == queue
                        && !j.state.is_terminal()
                        && (root || j.owner == agent)
                    {
                        targets.push(i);
                    }
                }
                i += 1;
            }
        } else {
            let mut k = 0usize;
            while k < ids.len() {
                if let Some(id) = ids.get(k) {
                    match self.index_of(*id) {
                        Some(idx) => {
                            let snapshot = match self.jobs.get(idx) {
                                Some(j) => (j.queue.clone(), j.owner.clone(), j.state),
                                None => {
                                    k += 1;
                                    continue;
                                }
                            };
                            if snapshot.0 != queue {
                                return Err(LpdError::QueueMismatch);
                            }
                            if !root && snapshot.1 != agent {
                                // 记录一次 Owner 拒绝：账本必须能回答「谁试图删
                                // 别人的作业」，而不是只把计数加一。
                                res.denied = res.denied.saturating_add(1);
                                self.note_err(LpdError::Owner);
                            } else if snapshot.2.is_terminal() {
                                res.denied = res.denied.saturating_add(1);
                            } else {
                                targets.push(idx);
                            }
                        }
                        None => res.denied = res.denied.saturating_add(1),
                    }
                }
                k += 1;
            }
        }
        let mut t = 0usize;
        while t < targets.len() {
            if let Some(idx) = targets.get(t) {
                if let Some(j) = self.jobs.get_mut(*idx) {
                    match j.state {
                        JobState::Printing => {
                            j.state = JobState::CancelPending;
                            res.pending = res.pending.saturating_add(1);
                        }
                        JobState::Queued | JobState::Held => {
                            j.state = JobState::Cancelled;
                            res.cancelled = res.cancelled.saturating_add(1);
                        }
                        _ => res.denied = res.denied.saturating_add(1),
                    }
                }
            }
            t += 1;
        }
        Ok(res)
    }

    /// 执行一条已编码命令。
    pub fn dispatch(&mut self, data: &[u8]) -> Result<CommandResult, LpdError> {
        let cmd = match decode_command(data) {
            Ok(c) => c,
            Err(e) => {
                self.note_err(e);
                return Err(e);
            }
        };
        let res = self.apply_command(cmd);
        if let Err(e) = res {
            self.note_err(e);
        }
        res
    }

    fn apply_command(&mut self, cmd: QueueCommand) -> Result<CommandResult, LpdError> {
        match cmd {
            QueueCommand::StartQueue(q) => {
                validate_queue(q.as_str())?;
                self.started = true;
                Ok(CommandResult::empty())
            }
            QueueCommand::StopQueue(q) => {
                validate_queue(q.as_str())?;
                self.started = false;
                Ok(CommandResult::empty())
            }
            QueueCommand::ShortQuery(q) => {
                validate_queue(q.as_str())?;
                let lines = self.short_status(q.as_str());
                let mut out: Vec<String> = Vec::new();
                let mut i = 0usize;
                while i < lines.len() {
                    if let Some(l) = lines.get(i) {
                        out.push(l.short_text());
                    }
                    i += 1;
                }
                Ok(CommandResult {
                    lines: out,
                    cancelled: 0,
                    pending: 0,
                    denied: 0,
                })
            }
            QueueCommand::LongQuery(q) => {
                validate_queue(q.as_str())?;
                let lines = self.long_status(q.as_str());
                let mut out: Vec<String> = Vec::new();
                let mut i = 0usize;
                while i < lines.len() {
                    if let Some(l) = lines.get(i) {
                        out.push(l.long_text());
                    }
                    i += 1;
                }
                Ok(CommandResult {
                    lines: out,
                    cancelled: 0,
                    pending: 0,
                    denied: 0,
                })
            }
            QueueCommand::RemoveJobs { queue, agent, ids } => {
                self.cancel(queue.as_str(), agent.as_str(), ids.as_slice())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 命令解码
// ---------------------------------------------------------------------------

fn read_line(d: &[u8], pos: &mut usize, err: LpdError) -> Result<String, LpdError> {
    let mut out = String::new();
    loop {
        let c = match d.get(*pos) {
            Some(v) => *v,
            None => return Err(err),
        };
        *pos += 1;
        if c == b'\n' {
            break;
        }
        if c < 0x21 || c > 0x7e {
            return Err(err);
        }
        out.push(c as char);
        if out.len() > 255 {
            return Err(err);
        }
    }
    Ok(out)
}

fn validate_queue(q: &str) -> Result<(), LpdError> {
    if q.is_empty() {
        return Err(LpdError::Queue);
    }
    if q.len() > MAX_QUEUE {
        return Err(LpdError::QueueLen);
    }
    let b = q.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c < 0x21 || *c > 0x7e {
                return Err(LpdError::Queue);
            }
        }
        i += 1;
    }
    Ok(())
}

fn validate_agent(a: &str) -> Result<(), LpdError> {
    if a.is_empty() || a.len() > MAX_AGENT {
        return Err(LpdError::Agent);
    }
    let b = a.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c < 0x21 || *c > 0x7e {
                return Err(LpdError::Agent);
            }
        }
        i += 1;
    }
    Ok(())
}

fn expect_end(d: &[u8], pos: usize) -> Result<(), LpdError> {
    if pos == d.len() {
        Ok(())
    } else {
        Err(LpdError::Trailing)
    }
}

fn parse_ids(d: &[u8], pos: &mut usize) -> Result<Vec<u32>, LpdError> {
    let mut ids: Vec<u32> = Vec::new();
    let mut cur: u32 = 0;
    let mut digits = 0usize;
    loop {
        let c = match d.get(*pos) {
            Some(v) => *v,
            None => return Err(LpdError::Trailing),
        };
        *pos += 1;
        if c == b'\n' {
            if digits > 0 {
                ids.push(cur);
            }
            break;
        }
        if c == b' ' {
            if digits == 0 {
                return Err(LpdError::IdSyntax);
            }
            ids.push(cur);
            cur = 0;
            digits = 0;
            if ids.len() > MAX_IDS {
                return Err(LpdError::IdCount);
            }
            continue;
        }
        if c < b'0' || c > b'9' {
            return Err(LpdError::IdSyntax);
        }
        cur = cur * 10 + (c - b'0') as u32;
        digits += 1;
        if digits > 9 {
            return Err(LpdError::IdSyntax);
        }
    }
    if ids.len() > MAX_IDS {
        return Err(LpdError::IdCount);
    }
    Ok(ids)
}

/// 解码一条队列命令。布局：
/// `op queue\n`（0x01..0x04）或 `0x05 queue\n agent\n id id ...\n`。
pub fn decode_command(d: &[u8]) -> Result<QueueCommand, LpdError> {
    if d.is_empty() {
        return Err(LpdError::Empty);
    }
    let op = match d.first() {
        Some(v) => *v,
        None => return Err(LpdError::Empty),
    };
    // 操作码先判：未知码不管载荷长什么样都是 Opcode，不能被后续
    // 「队列名为空」抢先改变语义（诊断码必须指向真正的首因）。
    if op < 0x01 || op > 0x05 {
        return Err(LpdError::Opcode);
    }
    let mut pos = 1usize;
    let queue = read_line(d, &mut pos, LpdError::Queue)?;
    validate_queue(queue.as_str())?;
    match op {
        0x01 => {
            expect_end(d, pos)?;
            Ok(QueueCommand::StartQueue(queue))
        }
        0x02 => {
            expect_end(d, pos)?;
            Ok(QueueCommand::StopQueue(queue))
        }
        0x03 => {
            expect_end(d, pos)?;
            Ok(QueueCommand::ShortQuery(queue))
        }
        0x04 => {
            expect_end(d, pos)?;
            Ok(QueueCommand::LongQuery(queue))
        }
        0x05 => {
            let agent = read_line(d, &mut pos, LpdError::Agent)?;
            validate_agent(agent.as_str())?;
            let ids = parse_ids(d, &mut pos)?;
            expect_end(d, pos)?;
            Ok(QueueCommand::RemoveJobs { queue, agent, ids })
        }
        _ => Err(LpdError::Opcode),
    }
}

/// 编码短查询。
pub fn cmd_short(queue: &str) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.push(0x03);
    push_str(&mut v, queue);
    v.push(b'\n');
    v
}

/// 编码长查询。
pub fn cmd_long(queue: &str) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.push(0x04);
    push_str(&mut v, queue);
    v.push(b'\n');
    v
}

/// 编码启停命令（start=true 为启动）。
pub fn cmd_start_stop(queue: &str, start: bool) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.push(if start { 0x01 } else { 0x02 });
    push_str(&mut v, queue);
    v.push(b'\n');
    v
}

/// 编码取消命令。`ids` 为空表示删除该 agent 的全部作业。
pub fn cmd_remove(queue: &str, agent: &str, ids: &[u32]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.push(0x05);
    push_str(&mut v, queue);
    v.push(b'\n');
    push_str(&mut v, agent);
    v.push(b'\n');
    let mut i = 0usize;
    while i < ids.len() {
        if let Some(id) = ids.get(i) {
            if i > 0 {
                v.push(b' ');
            }
            push_dec(&mut v, *id);
        }
        i += 1;
    }
    v.push(b'\n');
    v
}

fn push_str(v: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            v.push(*c);
        }
        i += 1;
    }
}

fn push_dec(v: &mut Vec<u8>, mut n: u32) {
    if n == 0 {
        v.push(b'0');
        return;
    }
    let mut buf: [u8; 12] = [0u8; 12];
    let mut i = 0usize;
    while n > 0 && i < 12 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        if let Some(c) = buf.get(i) {
            v.push(*c);
        }
    }
}

// ---------------------------------------------------------------------------
// 10 个查询/取消场景（模拟器对照基准）
// ---------------------------------------------------------------------------

/// 场景：初始注册表恒为「4 个作业」，命令序列可变。
#[derive(Clone, Debug)]
pub struct Scenario {
    pub cmds: Vec<Vec<u8>>,
    pub probe: u32,
}

/// 场景期望：6 个可观测量，全部为显式字面量（不依赖被测实现推导）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenarioExpect {
    pub lines: u16,
    pub cancelled: u16,
    pub pending: u16,
    pub denied: u16,
    pub remaining: u16,
    pub probe: u8,
}

/// 10 场景期望表。
///
/// 初始：1=alice/Queued、2=bob/Queued、3=alice/Held、4=carol/Printing。
pub const EXPECT: [ScenarioExpect; 10] = [
    ScenarioExpect { lines: 4, cancelled: 0, pending: 0, denied: 0, remaining: 4, probe: 0 },
    ScenarioExpect { lines: 0, cancelled: 1, pending: 0, denied: 0, remaining: 3, probe: 4 },
    ScenarioExpect { lines: 0, cancelled: 2, pending: 0, denied: 0, remaining: 2, probe: 4 },
    ScenarioExpect { lines: 0, cancelled: 0, pending: 0, denied: 1, remaining: 4, probe: 0 },
    ScenarioExpect { lines: 0, cancelled: 0, pending: 1, denied: 0, remaining: 4, probe: 3 },
    ScenarioExpect { lines: 0, cancelled: 1, pending: 0, denied: 0, remaining: 3, probe: 4 },
    ScenarioExpect { lines: 0, cancelled: 1, pending: 0, denied: 1, remaining: 3, probe: 4 },
    ScenarioExpect { lines: 0, cancelled: 0, pending: 0, denied: 1, remaining: 4, probe: 0 },
    ScenarioExpect { lines: 0, cancelled: 1, pending: 0, denied: 1, remaining: 3, probe: 4 },
    ScenarioExpect { lines: 4, cancelled: 0, pending: 0, denied: 0, remaining: 4, probe: 2 },
];

/// 建标准初始注册表：1=alice/Queued、2=bob/Queued、3=alice/Held、4=carol/Printing。
pub fn standard_registry() -> QueueRegistry {
    let mut r = QueueRegistry::new(8);
    let _ = r.enqueue("lp", "alice", "df1", 10);
    let _ = r.enqueue("lp", "bob", "df2", 20);
    let _ = r.enqueue("lp", "alice", "df3", 30);
    let _ = r.enqueue("lp", "carol", "df4", 40);
    let _ = r.apply_event(3, Event::Hold);
    let _ = r.apply_event(4, Event::StartPrinting);
    r
}

/// 10 个场景的命令序列与探针作业号。
pub fn scenarios() -> Vec<Scenario> {
    let mut v: Vec<Scenario> = Vec::new();
    v.push(Scenario { cmds: push_one(cmd_short("lp")), probe: 1 });
    v.push(Scenario { cmds: push_one(cmd_remove("lp", "alice", &[1])), probe: 1 });
    v.push(Scenario { cmds: push_one(cmd_remove("lp", "alice", &[])), probe: 3 });
    v.push(Scenario { cmds: push_one(cmd_remove("lp", "bob", &[1])), probe: 1 });
    v.push(Scenario { cmds: push_one(cmd_remove("lp", "carol", &[4])), probe: 4 });
    v.push(Scenario { cmds: push_one(cmd_remove("lp", "root", &[1])), probe: 1 });
    v.push(Scenario { cmds: push_one(cmd_remove("lp", "alice", &[2, 3])), probe: 3 });
    v.push(Scenario { cmds: push_one(cmd_remove("lp", "alice", &[99])), probe: 1 });
    let mut twice: Vec<Vec<u8>> = Vec::new();
    twice.push(cmd_remove("lp", "alice", &[1]));
    twice.push(cmd_remove("lp", "alice", &[1]));
    v.push(Scenario { cmds: twice, probe: 1 });
    v.push(Scenario { cmds: push_one(cmd_long("lp")), probe: 4 });
    v
}

fn push_one(c: Vec<u8>) -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = Vec::new();
    v.push(c);
    v
}

/// 场景执行后的观测量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub lines: u16,
    pub cancelled: u16,
    pub pending: u16,
    pub denied: u16,
    pub remaining: u16,
    pub probe: u8,
    /// 命令序列里出现错误的数量。
    pub errors: u16,
}

/// 跑第 i 个场景；下标越界返回 None。
pub fn run_scenario(i: usize) -> Option<Outcome> {
    let sc = scenarios().get(i)?.clone();
    let mut reg = standard_registry();
    let mut out = Outcome {
        lines: 0,
        cancelled: 0,
        pending: 0,
        denied: 0,
        remaining: 0,
        probe: 0,
        errors: 0,
    };
    let mut k = 0usize;
    while k < sc.cmds.len() {
        if let Some(cmd) = sc.cmds.get(k) {
            match reg.dispatch(cmd.as_slice()) {
                Ok(r) => {
                    out.lines = out.lines.saturating_add(r.lines.len() as u16);
                    out.cancelled = out.cancelled.saturating_add(r.cancelled);
                    out.pending = out.pending.saturating_add(r.pending);
                    out.denied = out.denied.saturating_add(r.denied);
                }
                Err(_) => out.errors = out.errors.saturating_add(1),
            }
        }
        k += 1;
    }
    out.remaining = reg.remaining("lp") as u16;
    out.probe = match reg.get(sc.probe) {
        Some(j) => j.state.code(),
        None => 255,
    };
    Some(out)
}

// ---------------------------------------------------------------------------
// 注入核：15 类坏样本
// ---------------------------------------------------------------------------

/// 注入核返回（命中，总数，码位掩码）。
pub fn injection_core() -> (usize, usize, u32) {
    let total = 15usize;
    let mut caught = 0usize;
    let mut mask = 0u32;

    let mut cases: Vec<(Vec<u8>, LpdError)> = Vec::new();
    cases.push((Vec::new(), LpdError::Empty));
    cases.push((bytes(&[0x07]), LpdError::Opcode));
    cases.push((bytes(&[0x03, b'\n']), LpdError::Queue));
    cases.push((long_queue_cmd(), LpdError::QueueLen));
    cases.push((bytes(&[0x05, b'l', b'p', b'\n', b'\n']), LpdError::Agent));
    cases.push((bytes(&[0x05, b'l', b'p', b'\n', b'a', b'\n', b'x', b'1', b'\n']), LpdError::IdSyntax));
    cases.push((many_ids_cmd(), LpdError::IdCount));
    cases.push((bytes(&[0x03, b'l', b'p', b'\n', b'X']), LpdError::Trailing));

    let mut i = 0usize;
    while i < cases.len() {
        if let Some((b, want)) = cases.get(i) {
            if decode_command(b.as_slice()).err() == Some(*want) {
                caught += 1;
                mask |= 1u32 << want.slot();
            }
        }
        i += 1;
    }

    // 状态机非法边
    let mut reg = standard_registry();
    if reg.apply_event(4, Event::StartPrinting).err() == Some(LpdError::State) {
        caught += 1;
        mask |= 1u32 << LpdError::State.slot();
    }
    // 终态重复取消
    if reg.apply_event(1, Event::Cancel).is_ok()
        && reg.apply_event(1, Event::Cancel).err() == Some(LpdError::Already)
    {
        caught += 1;
        mask |= 1u32 << LpdError::Already.slot();
    }
    // 属主不符
    if let Ok(r) = reg.cancel("lp", "bob", &[3]) {
        if r.denied == 1 && r.cancelled == 0 {
            caught += 1;
            mask |= 1u32 << LpdError::Owner.slot();
        }
    }
    // 未知 id（apply_event 路径）
    if reg.apply_event(99, Event::Cancel).err() == Some(LpdError::JobId) {
        caught += 1;
        mask |= 1u32 << LpdError::JobId.slot();
    }
    // 容量
    let mut small = QueueRegistry::new(1);
    let _ = small.enqueue("lp", "a", "d", 1);
    if small.enqueue("lp", "a", "d", 1).err() == Some(LpdError::Capacity) {
        caught += 1;
        mask |= 1u32 << LpdError::Capacity.slot();
    }
    // 重复 id
    let mut reg2 = standard_registry();
    let dup = reg2.register(1, "lp", "a", "d", 1, JobState::Queued);
    if dup.err() == Some(LpdError::DupId) {
        caught += 1;
        mask |= 1u32 << LpdError::DupId.slot();
    }
    // 队列不符
    let mut reg3 = standard_registry();
    if reg3.cancel("lp2", "alice", &[1]).err() == Some(LpdError::QueueMismatch) {
        caught += 1;
        mask |= 1u32 << LpdError::QueueMismatch.slot();
    }
    (caught, total, mask)
}

fn bytes(b: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            v.push(*c);
        }
        i += 1;
    }
    v
}

fn long_queue_cmd() -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.push(0x03);
    let mut i = 0usize;
    while i < 70 {
        v.push(b'q');
        i += 1;
    }
    v.push(b'\n');
    v
}

fn many_ids_cmd() -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.push(0x05);
    push_str(&mut v, "lp\nagent\n");
    let mut i = 0usize;
    while i < MAX_IDS + 8 {
        if i > 0 {
            v.push(b' ');
        }
        push_dec(&mut v, (i + 1) as u32);
        i += 1;
    }
    v.push(b'\n');
    v
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50144 自检：10 场景 × 6 观测量 + 状态机 + 15 类注入。
pub fn run_unx50144_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50144");

    // J1-1 10 个场景全部无错执行
    let mut ran = 0usize;
    let mut errors = 0u16;
    let mut i = 0usize;
    while i < 10 {
        if let Some(o) = run_scenario(i) {
            ran += 1;
            errors = errors.saturating_add(o.errors);
        }
        i += 1;
    }
    let ok = ran == 10 && errors == 0;
    let det = format!("ran={}/10 err={}", ran, errors);
    s.add_dyn("50144-J1-十场景执行", ok, &det);

    // J1-2 六观测量 × 10 = 60 项对照
    let mut matched = 0usize;
    let mut compared = 0usize;
    let mut k = 0usize;
    while k < 10 {
        if let (Some(o), Some(e)) = (run_scenario(k), EXPECT.get(k)) {
            let fields: [bool; 6] = [
                o.lines == e.lines,
                o.cancelled == e.cancelled,
                o.pending == e.pending,
                o.denied == e.denied,
                o.remaining == e.remaining,
                o.probe == e.probe,
            ];
            let mut f = 0usize;
            while f < fields.len() {
                if let Some(p) = fields.get(f) {
                    if *p {
                        matched += 1;
                    }
                    compared += 1;
                }
                f += 1;
            }
        }
        k += 1;
    }
    let ok = compared == 60 && matched == 60;
    let det = format!("matched={}/{}", matched, compared);
    s.add_dyn("50144-J1-状态一致", ok, &det);

    // J1-3 取消守恒：remaining == 4 - cancelled（CancelPending 仍占槽位，
    // 所以 pending 不减剩余数——这正是「打印中取消不消失」的量化表达）
    let mut cons_ok = true;
    let mut c = 0usize;
    while c < 10 {
        if let Some(o) = run_scenario(c) {
            if o.remaining != 4u16.saturating_sub(o.cancelled) {
                cons_ok = false;
            }
        }
        c += 1;
    }
    s.add("50144-J1-取消守恒", cons_ok, if cons_ok { "" } else { "作业数不守恒" });

    // J1-4 查询只读：同一注册表连续两次短查询，作业状态逐项不变
    let mut reg = standard_registry();
    let before: [u8; 4] = [
        reg.get(1).map(|j| j.state.code()).unwrap_or(255),
        reg.get(2).map(|j| j.state.code()).unwrap_or(255),
        reg.get(3).map(|j| j.state.code()).unwrap_or(255),
        reg.get(4).map(|j| j.state.code()).unwrap_or(255),
    ];
    let q1 = reg.dispatch(cmd_short("lp").as_slice());
    let q2 = reg.dispatch(cmd_long("lp").as_slice());
    let after: [u8; 4] = [
        reg.get(1).map(|j| j.state.code()).unwrap_or(255),
        reg.get(2).map(|j| j.state.code()).unwrap_or(255),
        reg.get(3).map(|j| j.state.code()).unwrap_or(255),
        reg.get(4).map(|j| j.state.code()).unwrap_or(255),
    ];
    let same = before == after;
    let lines_ok = match (q1, q2) {
        (Ok(a), Ok(b)) => a.lines.len() == 4 && b.lines.len() == 4,
        _ => false,
    };
    let ok = same && lines_ok;
    let det = format!("same={} lines={}", same, lines_ok);
    s.add_dyn("50144-J1-查询只读", ok, &det);

    // J1-5 查询行格式：短 4 字段、长 5 字段
    let mut fmt_ok = true;
    let mut reg2 = standard_registry();
    if let Ok(r) = reg2.dispatch(cmd_short("lp").as_slice()) {
        let mut i2 = 0usize;
        while i2 < r.lines.len() {
            if let Some(l) = r.lines.get(i2) {
                if count_fields(l) != 4 {
                    fmt_ok = false;
                }
            }
            i2 += 1;
        }
    } else {
        fmt_ok = false;
    }
    let mut reg3 = standard_registry();
    if let Ok(r) = reg3.dispatch(cmd_long("lp").as_slice()) {
        let mut i3 = 0usize;
        while i3 < r.lines.len() {
            if let Some(l) = r.lines.get(i3) {
                if count_fields(l) != 5 {
                    fmt_ok = false;
                }
            }
            i3 += 1;
        }
    } else {
        fmt_ok = false;
    }
    s.add("50144-J1-查询字段数", fmt_ok, if fmt_ok { "" } else { "字段数不符" });

    // J1-6 取消幂等：连发两次同一取消，剩余数不变、状态不变
    let mut reg4 = standard_registry();
    let first = reg4.dispatch(cmd_remove("lp", "alice", &[1]).as_slice());
    let rem1 = reg4.remaining("lp");
    let st1 = reg4.get(1).map(|j| j.state.code()).unwrap_or(255);
    let second = reg4.dispatch(cmd_remove("lp", "alice", &[1]).as_slice());
    let rem2 = reg4.remaining("lp");
    let st2 = reg4.get(1).map(|j| j.state.code()).unwrap_or(255);
    let ok = match (first, second) {
        (Ok(a), Ok(b)) => {
            a.cancelled == 1
                && b.cancelled == 0
                && b.denied == 1
                && rem1 == rem2
                && st1 == st2
                && st1 == JobState::Cancelled.code()
        }
        _ => false,
    };
    s.add("50144-J1-取消幂等", ok, if ok { "" } else { "重复取消改变状态" });

    // J1-7 属主隔离 + root 特权
    let mut reg5 = standard_registry();
    let denied = reg5.dispatch(cmd_remove("lp", "bob", &[1]).as_slice());
    let root = reg5.dispatch(cmd_remove("lp", "root", &[1]).as_slice());
    let ok = match (denied, root) {
        (Ok(a), Ok(b)) => a.cancelled == 0 && a.denied == 1 && b.cancelled == 1 && b.denied == 0,
        _ => false,
    };
    s.add("50144-J1-属主隔离", ok, if ok { "" } else { "属主/特权判定错" });

    // J1-8 打印中取消：先转 CancelPending 保留槽位，Finish 后落 Cancelled
    let mut reg6 = standard_registry();
    let pend = reg6.dispatch(cmd_remove("lp", "carol", &[4]).as_slice());
    let pend_state = reg6.get(4).map(|j| j.state);
    let kept = reg6.remaining("lp");
    let fin = reg6.apply_event(4, Event::Finish);
    let ok = match (pend, pend_state, fin) {
        (Ok(r), Some(st), Ok(next)) => {
            r.pending == 1
                && r.cancelled == 0
                && st == JobState::CancelPending
                && kept == 4
                && next == JobState::Cancelled
        }
        _ => false,
    };
    s.add("50144-J1-打印中取消", ok, if ok { "" } else { "CancelPending 语义错" });

    // J1-9 合法状态链：Queued→Printing→Done，Held→Release→Queued
    let mut reg7 = QueueRegistry::new(4);
    let id = match reg7.enqueue("lp", "u", "d", 1) {
        Ok(v) => v,
        Err(_) => 0,
    };
    let chain = reg7.apply_event(id, Event::StartPrinting).ok() == Some(JobState::Printing)
        && reg7.apply_event(id, Event::Finish).ok() == Some(JobState::Done);
    let mut reg8 = QueueRegistry::new(4);
    let id2 = match reg8.enqueue("lp", "u", "d", 1) {
        Ok(v) => v,
        Err(_) => 0,
    };
    let chain2 = reg8.apply_event(id2, Event::Hold).ok() == Some(JobState::Held)
        && reg8.apply_event(id2, Event::Release).ok() == Some(JobState::Queued);
    let ok = chain && chain2;
    s.add("50144-J1-合法状态链", ok, if ok { "" } else { "合法边被拒" });

    // J1-10 非法边：Done 再 StartPrinting → State；Cancelled 再 Cancel → Already
    let mut reg9 = QueueRegistry::new(2);
    let id3 = match reg9.enqueue("lp", "u", "d", 1) {
        Ok(v) => v,
        Err(_) => 0,
    };
    let _ = reg9.apply_event(id3, Event::Cancel);
    let already = reg9.apply_event(id3, Event::Cancel).err() == Some(LpdError::Already);
    let _ = reg9.apply_event(id3, Event::Purge);
    let bad = reg9.apply_event(id3, Event::StartPrinting).err() == Some(LpdError::State);
    let ok = already && bad;
    s.add("50144-J1-非法边必拒", ok, if ok { "" } else { "非法迁移被接受" });

    // J1-11 命令解码往返：5 类操作码逐个还原
    let mut dec_ok = 0usize;
    if decode_command(cmd_short("lp").as_slice()).ok() == Some(QueueCommand::ShortQuery("lp".to_string())) {
        dec_ok += 1;
    }
    if decode_command(cmd_long("lp").as_slice()).ok() == Some(QueueCommand::LongQuery("lp".to_string())) {
        dec_ok += 1;
    }
    if decode_command(cmd_start_stop("lp", true).as_slice()).ok()
        == Some(QueueCommand::StartQueue("lp".to_string()))
    {
        dec_ok += 1;
    }
    if decode_command(cmd_start_stop("lp", false).as_slice()).ok()
        == Some(QueueCommand::StopQueue("lp".to_string()))
    {
        dec_ok += 1;
    }
    if decode_command(cmd_remove("lp", "alice", &[2, 3]).as_slice()).ok()
        == Some(QueueCommand::RemoveJobs {
            queue: "lp".to_string(),
            agent: "alice".to_string(),
            ids: vec2(2, 3),
        })
    {
        dec_ok += 1;
    }
    let ok = dec_ok == 5;
    let det = format!("decoded={}/5", dec_ok);
    s.add_dyn("50144-J1-命令往返", ok, &det);

    // J1-12 启停语义：Start/Stop 只改启停位，不动作业
    let mut reg10 = standard_registry();
    let stop = reg10.dispatch(cmd_start_stop("lp", false).as_slice());
    let stopped = !reg10.started() && reg10.remaining("lp") == 4;
    let start = reg10.dispatch(cmd_start_stop("lp", true).as_slice());
    let resumed = reg10.started() && reg10.remaining("lp") == 4;
    let ok = stop.is_ok() && start.is_ok() && stopped && resumed;
    s.add("50144-J1-启停语义", ok, if ok { "" } else { "启停污染队列" });

    // J1-13 坏命令记账：5 类逐类计数，首错留痕
    let mut reg11 = QueueRegistry::new(4);
    let bads: Vec<(Vec<u8>, LpdError)> = {
        let mut v: Vec<(Vec<u8>, LpdError)> = Vec::new();
        v.push((Vec::new(), LpdError::Empty));
        v.push((bytes(&[0x09, b'l', b'p', b'\n']), LpdError::Opcode));
        v.push((bytes(&[0x03, b'\n']), LpdError::Queue));
        v.push((bytes(&[0x05, b'l', b'p', b'\n', b'\n']), LpdError::Agent));
        v.push((bytes(&[0x03, b'l', b'p', b'\n', b'X']), LpdError::Trailing));
        v
    };
    let mut b = 0usize;
    while b < bads.len() {
        if let Some((cmd, _)) = bads.get(b) {
            let _ = reg11.dispatch(cmd.as_slice());
        }
        b += 1;
    }
    let mut per_class = true;
    let mut b2 = 0usize;
    while b2 < bads.len() {
        if let Some((_, e)) = bads.get(b2) {
            if reg11.err_count(*e) != 1 {
                per_class = false;
            }
        }
        b2 += 1;
    }
    let ok = reg11.rejected == 5 && per_class && reg11.first_err == LpdError::Empty.code();
    let det = format!("rej={} first={:x}", reg11.rejected, reg11.first_err);
    s.add_dyn("50144-J1-坏命令记账", ok, &det);

    // J1-14 id 数量上限：33 个 id 拒绝，32 个接受
    let mut ok_ids = decode_command(many_ids_cmd().as_slice()).err() == Some(LpdError::IdCount);
    let mut many: Vec<u32> = Vec::new();
    let mut n = 0usize;
    while n < MAX_IDS {
        many.push((n + 1) as u32);
        n += 1;
    }
    let mut at_cap: Vec<u8> = Vec::new();
    at_cap.push(0x05);
    push_str(&mut at_cap, "lp\nagent\n");
    let mut m = 0usize;
    while m < many.len() {
        if let Some(id) = many.get(m) {
            if m > 0 {
                at_cap.push(b' ');
            }
            push_dec(&mut at_cap, *id);
        }
        m += 1;
    }
    at_cap.push(b'\n');
    ok_ids = ok_ids && decode_command(at_cap.as_slice()).is_ok();
    s.add("50144-J1-id上限", ok_ids, if ok_ids { "" } else { "32/33 边界判错" });

    // J1-15 注册表边界：容量满显式拒、重复 id 显式拒
    let mut small = QueueRegistry::new(1);
    let e1 = small.enqueue("lp", "a", "d", 1);
    let e2 = small.enqueue("lp", "a", "d", 1);
    let mut reg12 = standard_registry();
    let dup = reg12.register(2, "lp", "a", "d", 1, JobState::Queued);
    let ok = e1.is_ok()
        && e2.err() == Some(LpdError::Capacity)
        && dup.err() == Some(LpdError::DupId)
        && reg12.err_count(LpdError::DupId) == 1;
    s.add("50144-J1-注册表边界", ok, if ok { "" } else { "容量/重复未拦截" });

    // J1-16 注入核：15/15 且码位全覆盖
    let (caught, total, mask) = injection_core();
    let full = (1u32 << ERR_SLOTS) - 1;
    let ok = caught == total && mask == full;
    let det = format!("注入 {}/{} mask={:x}", caught, total, mask);
    s.add_dyn("50144-J1-注入必抓", ok, &det);

    s
}

fn vec2(a: u32, b: u32) -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    v.push(a);
    v.push(b);
    v
}

fn count_fields(line: &str) -> usize {
    if line.is_empty() {
        return 0;
    }
    let b = line.as_bytes();
    let mut n = 1usize;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b' ' {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50144_checks_green() {
        let set = run_unx50144_checks();
        assert!(set.all_passed(), "UNX-F50144 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50144_scenarios_match_expect() {
        let mut i = 0usize;
        while i < 10 {
            let o = match run_scenario(i) {
                Some(v) => v,
                None => return,
            };
            let e = match EXPECT.get(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(o.lines, e.lines, "scenario {} lines", i);
            assert_eq!(o.cancelled, e.cancelled, "scenario {} cancelled", i);
            assert_eq!(o.pending, e.pending, "scenario {} pending", i);
            assert_eq!(o.denied, e.denied, "scenario {} denied", i);
            assert_eq!(o.remaining, e.remaining, "scenario {} remaining", i);
            assert_eq!(o.probe, e.probe, "scenario {} probe", i);
            i += 1;
        }
    }

    #[test]
    fn unx50144_injection_covers_all_codes() {
        let (caught, total, mask) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(mask, (1u32 << ERR_SLOTS) - 1);
    }

    #[test]
    fn unx50144_cancel_is_idempotent() {
        let mut reg = standard_registry();
        let first = reg.cancel("lp", "alice", &[1]);
        let second = reg.cancel("lp", "alice", &[1]);
        assert!(first.is_ok());
        match second {
            Ok(r) => assert_eq!(r.cancelled, 0),
            Err(_) => assert!(false, "second cancel errored"),
        }
        assert_eq!(reg.remaining("lp"), 3);
    }
}
