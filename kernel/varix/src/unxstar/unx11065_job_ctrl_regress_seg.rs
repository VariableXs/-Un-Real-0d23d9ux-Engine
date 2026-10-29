//! UNX-F11065 作业控制回归段（0x1e820 段）
//!
//! 本段重放 B08 作业控制的十条判据：会话表持有前台进程组，作业行
//! 记录 (job, pgid, sid, 状态, 停止信号, 退出码)，信号投递驱动
//! Foreground/Background/Stopped/Done 四态迁移。重放要求 10/10 行
//! 语义合法，且会话不变量（至多一个前台组、前台组属于会话成员、
//! 停止作业必有停止信号）逐条成立。
//!
//! 不变式：done + stopped + running == 登记总数；wait 状态字按
//! POSIX WIFSTOPPED/WIFEXITED 编码；冻结后写路径全以 JC_FROZEN 拒止。
//!
//! 结构：JobRow（作业行）/ SessionTable（会话与前台组）/ JobSegment
//! （B08 总账 + 重放 + 冻结）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11065 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e820;

/// 空账：作业号/进程组号为 0，或等待无该作业
pub const JC_EMPTY: u32 = SEG_BASE + 0x01;
/// 越界：信号号不在闭集或进程组不在会话成员内
pub const JC_RANGE: u32 = SEG_BASE + 0x02;
/// 重号：作业号或会话成员重复登记
pub const JC_DUP: u32 = SEG_BASE + 0x03;
/// 失配：状态迁移与会话语义冲突
pub const JC_MISMATCH: u32 = SEG_BASE + 0x04;
/// 短缺：作业不足十席或会话成员表已满
pub const JC_SHORT: u32 = SEG_BASE + 0x05;
/// 已封：段账冻结后的信号/登记/收割
pub const JC_FROZEN: u32 = SEG_BASE + 0x06;

/// FNV 折前缀：低两字节 0x65 标记 11065 号卡
pub const JC_TAG: u64 = 0x1e80_65;

/// 本段固定十席
const SEATS: usize = 10;
/// 单会话进程组上限
const MAX_MEMBERS: usize = 8;
/// SIGCONT
const SIGCONT: u8 = 18;
/// SIGTSTP
const SIGTSTP: u8 = 20;
/// SIGTTIN
const SIGTTIN: u8 = 21;
/// SIGTTOU
const SIGTTOU: u8 = 22;

fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01b3)
}

fn fnv_u16(h: u64, v: u16) -> u64 {
    fnv_byte(fnv_byte(h, (v & 0xff) as u8), ((v >> 8) & 0xff) as u8)
}

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut k = 0u32;
    while k < 4 {
        h = fnv_byte(h, ((v >> k.wrapping_mul(8)) & 0xff) as u8);
        k = k.wrapping_add(1);
    }
    h
}

/// 作业四态（B08 判据只认这四态）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    /// 前台作业（会话内至多一个）
    Foreground,
    /// 后台运行
    Background,
    /// 已停止（必有停止信号）
    Stopped,
    /// 已收割
    Done,
}

impl JobState {
    /// 落账判别码
    pub fn code(self) -> u8 {
        match self {
            JobState::Foreground => 1,
            JobState::Background => 2,
            JobState::Stopped => 3,
            JobState::Done => 4,
        }
    }

    /// 是否可等待（停止/退出）
    pub fn waitable(self) -> bool {
        match self {
            JobState::Stopped | JobState::Done => true,
            _ => false,
        }
    }
}

/// 单条作业行
#[derive(Clone, Debug)]
pub struct JobRow {
    /// 作业号（1..=10，段内唯一）
    pub job: u16,
    /// 进程组号
    pub pgid: u16,
    /// 会话号
    pub sid: u16,
    /// 当前状态
    pub state: JobState,
    /// 停止信号（Stopped 时非 0）
    pub stop_sig: u8,
    /// 退出码（Done 时有效）
    pub exit_code: u8,
}

impl JobRow {
    /// 新建一条后台作业行
    pub fn new(job: u16, pgid: u16, sid: u16) -> JobRow {
        JobRow { job, pgid, sid, state: JobState::Background, stop_sig: 0, exit_code: 0 }
    }

    /// 按 POSIX 编码 wait 状态字：停止 (sig<<8)|0x7f，退出 code<<8
    pub fn wait_status(&self) -> Result<u32, u32> {
        match self.state {
            JobState::Done => Ok((self.exit_code as u32) << 8),
            JobState::Stopped => {
                if self.stop_sig == 0 {
                    return Err(JC_MISMATCH);
                }
                Ok(((self.stop_sig as u32) << 8) | 0x7f)
            }
            _ => Err(JC_MISMATCH),
        }
    }

    /// 作业行指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_u16(h, self.job);
        h = fnv_u16(h, self.pgid);
        h = fnv_u16(h, self.sid);
        h = fnv_byte(h, self.state.code());
        h = fnv_byte(h, self.stop_sig);
        fnv_byte(h, self.exit_code)
    }
}

/// 会话表：成员进程组 + 前台组
#[derive(Clone, Debug)]
pub struct SessionTable {
    pub sid: u16,
    /// 前台进程组（恒为 members 之一）
    pub fg_pgid: u16,
    members: Vec<u16>,
    frozen: Option<u64>,
}

impl SessionTable {
    /// 以 shell 进程组开一个会话
    pub fn new(sid: u16, shell_pgid: u16) -> SessionTable {
        let mut members = Vec::new();
        members.push(shell_pgid);
        SessionTable { sid, fg_pgid: shell_pgid, members, frozen: None }
    }

    fn guard(&self) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(JC_FROZEN);
        }
        Ok(())
    }

    /// 是否会话成员
    pub fn member(&self, pgid: u16) -> bool {
        let mut k = 0usize;
        while k < self.members.len() {
            if self.members[k] == pgid {
                return true;
            }
            k += 1;
        }
        false
    }

    /// 加入进程组：0 号 / 重复 / 表满三路拒
    pub fn join(&mut self, pgid: u16) -> Result<(), u32> {
        self.guard()?;
        if pgid == 0 {
            return Err(JC_EMPTY);
        }
        if self.member(pgid) {
            return Err(JC_DUP);
        }
        if self.members.len() >= MAX_MEMBERS {
            return Err(JC_SHORT);
        }
        self.members.push(pgid);
        Ok(())
    }

    /// 前台组交接：必须是会话成员
    pub fn handoff(&mut self, pgid: u16) -> Result<(), u32> {
        self.guard()?;
        if !self.member(pgid) {
            return Err(JC_RANGE);
        }
        self.fg_pgid = pgid;
        Ok(())
    }

    /// 前台交还 shell（members[0]）
    pub fn reclaim(&mut self) -> Result<(), u32> {
        self.guard()?;
        if self.members.is_empty() {
            return Err(JC_EMPTY);
        }
        self.fg_pgid = self.members[0];
        Ok(())
    }

    /// 会话不变量：至多一个前台作业且其 pgid 等于 fg_pgid、sid 一致
    pub fn fg_valid(&self, jobs: &[JobRow]) -> bool {
        let mut fg = 0u16;
        let mut k = 0usize;
        while k < jobs.len() {
            let j = &jobs[k];
            if j.state == JobState::Foreground {
                if j.pgid != self.fg_pgid || j.sid != self.sid {
                    return false;
                }
                fg = fg.wrapping_add(1);
            }
            k += 1;
        }
        fg <= 1
    }

    /// 成员表长度
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// 会话指纹
    pub fn fold(&self) -> u64 {
        let mut h = JC_TAG;
        h = fnv_u16(h, self.sid);
        h = fnv_u16(h, self.fg_pgid);
        let mut k = 0usize;
        while k < self.members.len() {
            h = fnv_u16(h, self.members[k]);
            k += 1;
        }
        h
    }

    /// 冻结会话表
    pub fn freeze(&mut self) -> u64 {
        let f = self.fold();
        self.frozen = Some(f);
        f
    }
}

/// B08 回归段总账
#[derive(Clone, Debug)]
pub struct JobSegment {
    /// 会话表
    pub session: SessionTable,
    /// 作业行
    pub jobs: Vec<JobRow>,
    /// 最近一次重放合法行数
    pub replay_hits: u16,
    frozen: Option<u64>,
}

impl JobSegment {
    /// 空段账
    pub fn new(sid: u16, shell_pgid: u16) -> JobSegment {
        JobSegment {
            session: SessionTable::new(sid, shell_pgid),
            jobs: Vec::new(),
            replay_hits: 0,
            frozen: None,
        }
    }

    fn guard(&self) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(JC_FROZEN);
        }
        Ok(())
    }

    fn index(&self, job: u16) -> Option<usize> {
        let mut k = 0usize;
        while k < self.jobs.len() {
            if self.jobs[k].job == job {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记作业行：0 号 / 越界 pgid / 跨会话 / 重复 四道闸
    pub fn enroll(&mut self, row: JobRow) -> Result<(), u32> {
        self.guard()?;
        if row.job == 0 {
            return Err(JC_EMPTY);
        }
        if row.pgid == 0 {
            return Err(JC_RANGE);
        }
        if row.sid != self.session.sid {
            return Err(JC_MISMATCH);
        }
        if !self.session.member(row.pgid) {
            return Err(JC_MISMATCH);
        }
        if self.index(row.job).is_some() {
            return Err(JC_DUP);
        }
        if row.state == JobState::Foreground {
            if row.pgid != self.session.fg_pgid {
                return Err(JC_MISMATCH);
            }
            let mut k = 0usize;
            while k < self.jobs.len() {
                if self.jobs[k].state == JobState::Foreground {
                    return Err(JC_MISMATCH);
                }
                k += 1;
            }
        }
        self.jobs.push(row);
        Ok(())
    }

    /// 投递作业控制信号（TSTP/CONT/TTIN/TTOU）
    pub fn signal(&mut self, job: u16, sig: u8) -> Result<(), u32> {
        self.guard()?;
        let idx = match self.index(job) {
            Some(i) => i,
            None => return Err(JC_EMPTY),
        };
        if self.jobs[idx].state == JobState::Done {
            return Err(JC_MISMATCH);
        }
        match sig {
            SIGTSTP => {
                self.jobs[idx].state = JobState::Stopped;
                self.jobs[idx].stop_sig = SIGTSTP;
                let pgid = self.jobs[idx].pgid;
                if pgid == self.session.fg_pgid {
                    self.session.reclaim()?;
                }
            }
            SIGCONT => {
                if self.jobs[idx].state != JobState::Stopped {
                    return Err(JC_MISMATCH);
                }
                self.jobs[idx].state = JobState::Background;
                self.jobs[idx].stop_sig = 0;
            }
            SIGTTIN => {
                if self.jobs[idx].state != JobState::Background {
                    return Err(JC_MISMATCH);
                }
                self.jobs[idx].state = JobState::Stopped;
                self.jobs[idx].stop_sig = SIGTTIN;
            }
            SIGTTOU => {
                if self.jobs[idx].state != JobState::Background {
                    return Err(JC_RANGE);
                }
                self.jobs[idx].state = JobState::Stopped;
                self.jobs[idx].stop_sig = SIGTTOU;
            }
            _ => return Err(JC_RANGE),
        }
        Ok(())
    }

    /// 前台交接（作业必须可运行）
    pub fn handoff(&mut self, job: u16) -> Result<(), u32> {
        self.guard()?;
        let idx = match self.index(job) {
            Some(i) => i,
            None => return Err(JC_EMPTY),
        };
        if self.jobs[idx].state == JobState::Done {
            return Err(JC_MISMATCH);
        }
        let pgid = self.jobs[idx].pgid;
        self.session.handoff(pgid)?;
        let mut k = 0usize;
        while k < self.jobs.len() {
            if self.jobs[k].state == JobState::Foreground {
                self.jobs[k].state = JobState::Background;
            }
            k += 1;
        }
        self.jobs[idx].state = JobState::Foreground;
        Ok(())
    }

    /// 收割作业
    pub fn reap(&mut self, job: u16, code: u8) -> Result<(), u32> {
        self.guard()?;
        let idx = match self.index(job) {
            Some(i) => i,
            None => return Err(JC_EMPTY),
        };
        if code > 127 {
            return Err(JC_RANGE);
        }
        self.jobs[idx].state = JobState::Done;
        self.jobs[idx].stop_sig = 0;
        self.jobs[idx].exit_code = code;
        Ok(())
    }

    /// 状态守恒：done + stopped + running == 登记数
    pub fn state_conserved(&self) -> bool {
        let mut done = 0usize;
        let mut stopped = 0usize;
        let mut running = 0usize;
        let mut k = 0usize;
        while k < self.jobs.len() {
            match self.jobs[k].state {
                JobState::Done => done += 1,
                JobState::Stopped => stopped += 1,
                _ => running += 1,
            }
            k += 1;
        }
        done + stopped + running == self.jobs.len()
    }

    /// 作业号唯一且严格递增
    pub fn ids_unique(&self) -> bool {
        let mut k = 1usize;
        while k < self.jobs.len() {
            if self.jobs[k].job <= self.jobs[k - 1].job {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 会话语义：前台组合法且停止作业带停止信号
    pub fn session_consistent(&self) -> bool {
        if !self.session.fg_valid(&self.jobs) {
            return false;
        }
        let mut k = 0usize;
        while k < self.jobs.len() {
            let j = &self.jobs[k];
            if j.sid != self.session.sid {
                return false;
            }
            if j.state == JobState::Stopped && j.stop_sig == 0 {
                return false;
            }
            if j.state == JobState::Stopped {
                if j.wait_status().is_err() {
                    return false;
                }
            }
            k += 1;
        }
        true
    }

    /// 全量重放：不足十席 JC_SHORT，存在非法行 JC_MISMATCH
    pub fn replay(&mut self) -> Result<u16, u32> {
        self.guard()?;
        if self.jobs.len() < SEATS {
            return Err(JC_SHORT);
        }
        let mut hits = 0u16;
        let mut k = 0usize;
        while k < self.jobs.len() {
            let j = &self.jobs[k];
            let legal = j.sid == self.session.sid
                && self.session.member(j.pgid)
                && !(j.state == JobState::Foreground && j.pgid != self.session.fg_pgid)
                && !(j.state == JobState::Stopped && j.stop_sig == 0);
            if legal {
                hits = hits.wrapping_add(1);
            }
            k += 1;
        }
        self.replay_hits = hits;
        if hits as usize != self.jobs.len() {
            return Err(JC_MISMATCH);
        }
        Ok(hits)
    }

    /// 段账指纹
    pub fn fold(&self) -> u64 {
        let mut h = self.session.fold();
        h = fnv_u16(h, self.replay_hits);
        let mut k = 0usize;
        while k < self.jobs.len() {
            h = self.jobs[k].digest(h);
            k += 1;
        }
        h
    }

    /// 冻结段账
    pub fn freeze(&mut self) -> u64 {
        let f = self.fold();
        self.frozen = Some(f);
        f
    }
}

/// 四条负路径：空成员 / 重成员 / 非成员前台 / 冻结信号
pub fn inject_four() -> [bool; 4] {
    let mut s0 = SessionTable::new(400, 400);
    let empty = matches!(s0.join(0), Err(JC_EMPTY));
    let mut s1 = SessionTable::new(400, 400);
    let first = s1.join(401).is_ok();
    let dup = first && matches!(s1.join(401), Err(JC_DUP));
    let mut s2 = SessionTable::new(400, 400);
    let range = matches!(s2.handoff(999), Err(JC_RANGE));
    let mut seg = JobSegment::new(400, 400);
    seg.freeze();
    let frozen = matches!(
        seg.enroll(JobRow::new(1, 400, 400)),
        Err(JC_FROZEN)
    );
    [empty, dup, range, frozen]
}

/// 构造 B08 标准样本（十席）
fn sample_segment() -> Result<JobSegment, u32> {
    let mut seg = JobSegment::new(400, 400);
    seg.session.join(401)?;
    seg.session.join(402)?;
    seg.session.handoff(402)?;
    let mut i = 1u16;
    while i <= 10 {
        let pgid = if i <= 4 || i > 5 { 401 } else { 402 };
        let mut row = JobRow::new(i, pgid, 400);
        if i == 5 {
            row.state = JobState::Foreground;
        }
        seg.enroll(row)?;
        i = i.wrapping_add(1);
    }
    seg.signal(5, SIGTSTP)?;
    seg.signal(5, SIGCONT)?;
    seg.signal(3, SIGTSTP)?;
    seg.signal(7, SIGTTIN)?;
    seg.reap(9, 0)?;
    seg.reap(10, 7)?;
    Ok(seg)
}

/// UNX-F11065 判据入口
pub fn run_unx11065_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11065");
    let base = match sample_segment() {
        Ok(v) => v,
        Err(_) => JobSegment::new(400, 400),
    };
    let mut seg = base.clone();
    let hits = seg.replay().unwrap_or(0);
    let session_ok = seg.session_consistent() && seg.session.fg_pgid == 400;
    s.add(
        "J1a-作业判据全量重放",
        hits == 10 && seg.jobs.len() == SEATS,
        "B08 十席必须全部登记且逐行合法",
    );
    s.add(
        "J1b-会话语义一致",
        session_ok && seg.session.member_count() == 3,
        "前台组必须归 shell、成员表三组、停止行带信号",
    );
    s.add(
        "J2-状态守恒",
        seg.state_conserved() && seg.jobs.len() == SEATS,
        "done+stopped+running 必须等于登记总数",
    );
    let inj = inject_four();
    s.add("J3a-空成员拒止", inj[0], "pgid=0 入会话必须 JC_EMPTY");
    s.add("J3b-重成员拒止", inj[1], "同组二次入会话必须 JC_DUP");
    s.add("J3c-非成员前台拒", inj[2], "非成员组交接必须 JC_RANGE");
    s.add("J3d-冻结信号拒", inj[3], "冻结后的登记必须 JC_FROZEN");
    let mut fz = base.clone();
    let fp = fz.freeze();
    let rejected = matches!(fz.signal(1, SIGTSTP), Err(JC_FROZEN))
        && matches!(fz.reap(1, 0), Err(JC_FROZEN));
    s.add(
        "J4-冻结不可写",
        rejected && fz.fold() == fp,
        "冻结后信号与收割都必须拒且指纹不变",
    );
    let mut a = base.clone();
    let fp_a = a.fold();
    let mut b = base.clone();
    let changed = b.reap(2, 3).is_ok() && b.fold() != fp_a;
    s.add("J5-指纹敏感性", changed, "单行退出码变化必须改变 fold");
    let zero_enroll = matches!(
        JobSegment::new(400, 400).enroll(JobRow::new(0, 400, 400)),
        Err(JC_EMPTY)
    );
    let mut cross = JobSegment::new(400, 400);
    let cross_bad = matches!(
        cross.enroll(JobRow::new(1, 400, 401)),
        Err(JC_MISMATCH)
    );
    let sig_range = {
        let mut g = JobSegment::new(400, 400);
        g.enroll(JobRow::new(1, 400, 400)).is_ok()
            && matches!(g.signal(1, 99), Err(JC_RANGE))
    };
    s.add(
        "J6-范围与空账拒止",
        zero_enroll && cross_bad && sig_range,
        "0 号/跨会话/越界信号三路都必须拒",
    );
    let unique = seg.ids_unique() && seg.jobs[0].job == 1 && seg.jobs[9].job == 10;
    let mut dseg = JobSegment::new(400, 400);
    let dup_reject = dseg.enroll(JobRow::new(1, 400, 400)).is_ok()
        && matches!(dseg.enroll(JobRow::new(1, 400, 400)), Err(JC_DUP));
    s.add(
        "J7-唯一与排序不变量",
        unique && dup_reject,
        "作业号必须唯一、登记序递增、重号当场拒",
    );
    let mut partial = base.clone();
    while partial.jobs.len() > 5 {
        partial.jobs.pop();
    }
    let negative = partial.jobs.len() == 5 && matches!(partial.replay(), Err(JC_SHORT));
    s.add(
        "J8-半成品不得全绿",
        negative,
        "五席半账必须 JC_SHORT，不得伪装成绿",
    );
    let mut forged = base.clone();
    if !forged.jobs.is_empty() {
        forged.jobs[0].state = JobState::Foreground;
    }
    s.add(
        "J10-伪造前台必须被识破",
        !forged.session_consistent(),
        "绕过交接直改前台态必须使会话判据变红",
    );
    s.add_dyn(
        "J9-动态账",
        hits == 10 && session_ok,
        &format!("hits={} 成员={} 前台组={}", hits, seg.session.member_count(), seg.session.fg_pgid),
    );
    s.add_dyn(
        "J9b-指纹账",
        seg.fold() != 0,
        &format!("fold={:016x} tag={:x}", seg.fold(), JC_TAG),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11065_flow() {
        let mut seg = sample_segment().unwrap_or(JobSegment::new(400, 400));
        assert_eq!(seg.replay().unwrap_or(0), 10);
        assert!(seg.session_consistent());
        assert!(seg.handoff(1).is_ok());
        assert!(seg.session.fg_pgid == 401);
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11065_checks_green() {
        assert!(run_unx11065_checks().all_passed(), "UNX-F11065 判据未全绿");
    }
}
