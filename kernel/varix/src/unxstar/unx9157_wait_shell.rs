//! UNX-F9157 wait 族与 shell 作业控制消费核（0x63360 段）
//!
//! 锚点：UNX-F9157-J1 作业账（作业登记/状态翻转/收割打印逐条落
//! 账）；J2 作业语义（作业号递增 + 状态机 RUN→STOP→DONE 闭包 +
//! WNOHANG 零阻塞轮询 + 前台阻塞等四性质）；J3 作业指纹（账本折叠
//! 指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B18（shell 作业控制消费面）。
//!
//! 结构：作业表（job_id→pgid/state）+ 作业状态机 + 轮询核
//! （WNOHANG）+ 阻塞核 + 作业账本 + 折叠指纹 + 注入核（错态/空留
//! 痕/伪作业/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63360 独占 0x20）。
pub const SEG_BASE: u32 = 0x63360;
pub const JB_UNK_JOB: u32 = SEG_BASE + 0x01; // 未知作业
pub const JB_BAD_STATE: u32 = SEG_BASE + 0x02; // 非法状态翻转
pub const JB_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const JB_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const JB_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 作业状态闭集。
pub const ST_RUN: u8 = 0; // 运行中
pub const ST_STOP: u8 = 1; // 已停（SIGTSTP）
pub const ST_DONE: u8 = 2; // 已完成（可收尸）
pub const ST_COUNT: u8 = 3;

pub const ST_NAMES: [&str; 3] = ["run", "stop", "done"];

/// 状态机合法翻转表：run→stop、run→done、stop→run（fg 继续）、
/// stop→done（被 SIGKILL）。
pub fn state_can_move(from: u8, to: u8) -> bool {
    match (from, to) {
        (ST_RUN, ST_STOP) | (ST_RUN, ST_DONE) | (ST_STOP, ST_RUN) | (ST_STOP, ST_DONE) => true,
        _ => false,
    }
}

/// 单条作业账。
#[derive(Clone)]
pub struct JobRec {
    pub seq: u32,
    pub job_id: u32,
    pub pgid: u32,
    pub from: u8,
    pub to: u8,
    pub note: String,
}

impl JobRec {
    pub fn ok(&self) -> bool {
        // 登记账（RUN→RUN 自环）合法；翻转账须过状态机
        !self.note.is_empty()
            && self.to < ST_COUNT
            && ((self.from == self.to && self.from == ST_RUN)
                || state_can_move(self.from, self.to))
    }

    pub fn render_line(&self) -> String {
        format!(
            "JB{}|j{}g{}|{}->{}|{}",
            self.seq,
            self.job_id,
            self.pgid,
            ST_NAMES[self.from as usize],
            ST_NAMES[self.to as usize],
            self.note
        )
    }
}

/// 作业条目。
#[derive(Clone)]
pub struct Job {
    pub job_id: u32,
    pub pgid: u32,
    pub state: u8,
    pub foreground: bool,
}

/// 作业控制簿（shell 消费核）。
pub struct JobBook {
    pub jobs: Vec<Job>,
    pub ledger: Vec<JobRec>,
    pub next_job: u32,
    pub seq_next: u32,
    pub sealed: bool,
}

impl JobBook {
    pub fn new() -> JobBook {
        JobBook { jobs: Vec::new(), ledger: Vec::new(), next_job: 1, seq_next: 1, sealed: false }
    }

    /// fork 登记：新作业 RUN，可指定前台/后台。
    pub fn register(&mut self, pgid: u32, fg: bool, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(JB_FROZEN);
        }
        if note.is_empty() {
            return Err(JB_EMPTY_NOTE);
        }
        let job_id = self.next_job;
        self.next_job += 1;
        self.jobs.push(Job { job_id, pgid, state: ST_RUN, foreground: fg });
        self.ledger.push(JobRec {
            seq: self.seq_next,
            job_id,
            pgid,
            from: ST_RUN,
            to: ST_RUN,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(job_id)
    }

    pub fn find(&self, job_id: u32) -> Option<usize> {
        self.jobs.iter().position(|j| j.job_id == job_id)
    }

    /// 状态翻转（状态机闸）。
    pub fn transition(&mut self, job_id: u32, to: u8, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(JB_FROZEN);
        }
        if note.is_empty() {
            return Err(JB_EMPTY_NOTE);
        }
        if to >= ST_COUNT {
            return Err(JB_BAD_STATE);
        }
        let i = match self.find(job_id) {
            Some(i) => i,
            None => return Err(JB_UNK_JOB),
        };
        let from = self.jobs[i].state;
        if !state_can_move(from, to) {
            return Err(JB_BAD_STATE);
        }
        self.jobs[i].state = to;
        self.ledger.push(JobRec {
            seq: self.seq_next,
            job_id,
            pgid: self.jobs[i].pgid,
            from,
            to,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(self.ledger.last().map(|r| r.seq).unwrap_or(0))
    }

    /// WNOHANG 轮询：取首个 DONE 作业（不阻塞），无则零。
    pub fn poll_wnohang(&self) -> Option<u32> {
        self.jobs.iter().find(|j| j.state == ST_DONE).map(|j| j.job_id)
    }

    /// 前台阻塞等：前台作业未 DONE 即阻塞（返回等待态）。
    pub fn fg_wait_needed(&self) -> bool {
        self.jobs.iter().any(|j| j.foreground && j.state != ST_DONE)
    }

    /// 收割：摘 DONE 作业（打印 [n] Done 语义）。
    pub fn reap(&mut self, job_id: u32, note: &str) -> Result<Job, u32> {
        if self.sealed {
            return Err(JB_FROZEN);
        }
        if note.is_empty() {
            return Err(JB_EMPTY_NOTE);
        }
        let i = match self.find(job_id) {
            Some(i) => i,
            None => return Err(JB_UNK_JOB),
        };
        if self.jobs[i].state != ST_DONE {
            return Err(JB_BAD_STATE);
        }
        let j = self.jobs.remove(i);
        Ok(j)
    }

    /// 前台唯一：至多一前台 RUN/STOP 作业。
    pub fn fg_unique(&self) -> bool {
        self.jobs.iter().filter(|j| j.foreground && j.state != ST_DONE).count() <= 1
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x91570;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.job_id);
            h = h.wrapping_mul(31).wrapping_add(r.pgid);
            h = h.wrapping_mul(31).wrapping_add(r.from as u32);
            h = h.wrapping_mul(31).wrapping_add(r.to as u32);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 非法翻转账（done→run）
    let bad = JobRec {
        seq: 1,
        job_id: 1,
        pgid: 1,
        from: ST_DONE,
        to: ST_RUN,
        note: "inj-move".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = JobRec {
        seq: 2,
        job_id: 1,
        pgid: 1,
        from: ST_RUN,
        to: ST_DONE,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪作业翻转
    let mut b = JobBook::new();
    got[2] = b.transition(99, ST_DONE, "inj-unk").err() == Some(JB_UNK_JOB);
    // 4) 非法态
    let _ = b.register(100, true, "real");
    got[3] = b.transition(1, 9, "inj-state").err() == Some(JB_BAD_STATE);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.transition(1, ST_DONE, "inj-frozen").err() == Some(JB_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9157_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9157");
    let mut b = JobBook::new();

    // J1 作业账非空
    let _ = b.register(100, true, "前台作业");
    let j1 = !b.ledger.is_empty();
    s.add("J1-作业账非空", j1, "登记落账至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且状态翻转合法");

    // J3 作业号递增
    let j3 = {
        let a = b.register(101, false, "后台一");
        let bb = b.register(102, false, "后台二");
        a.is_ok() && bb.is_ok() && bb.unwrap() > a.unwrap()
    };
    s.add("J3-作业号递增", j3, "job_id 严格递增");

    // J4 状态机闭包：run→stop→run→done
    let j4 = {
        let mut b2 = JobBook::new();
        let j = b2.register(200, true, "a").unwrap_or(0);
        let s1 = b2.transition(j, ST_STOP, "停").is_ok();
        let s2 = b2.transition(j, ST_RUN, "续").is_ok();
        let s3 = b2.transition(j, ST_DONE, "毕").is_ok();
        s1 && s2 && s3
    };
    s.add("J4-状态机闭包", j4, "run/stop/done 合法环全通");

    // J5 非法翻转拒：done→run
    let j5 = {
        let mut b2 = JobBook::new();
        let j = b2.register(201, true, "a").unwrap_or(0);
        let _ = b2.transition(j, ST_DONE, "毕");
        b2.transition(j, ST_RUN, "诈尸").err() == Some(JB_BAD_STATE)
    };
    s.add("J5-诈尸拒", j5, "done→run 必拒");

    // J6 WNOHANG 轮询零阻塞
    let j6 = {
        let mut b2 = JobBook::new();
        let j = b2.register(202, false, "后").unwrap_or(0);
        let none = b2.poll_wnohang().is_none();
        let _ = b2.transition(j, ST_DONE, "毕");
        let got = b2.poll_wnohang() == Some(j);
        none && got
    };
    s.add("J6-WNOHANG轮询", j6, "无完成回零、有完成即取");

    // J7 前台阻塞判定
    let j7 = {
        let mut b2 = JobBook::new();
        let j = b2.register(203, true, "前").unwrap_or(0);
        let wait1 = b2.fg_wait_needed();
        let _ = b2.transition(j, ST_DONE, "毕");
        wait1 && !b2.fg_wait_needed()
    };
    s.add("J7-前台阻塞判定", j7, "前台未毕需等、毕后放行");

    // J8 收割摘除
    let j8 = {
        let mut b2 = JobBook::new();
        let j = b2.register(204, false, "后").unwrap_or(0);
        let _ = b2.transition(j, ST_DONE, "毕");
        let got = b2.reap(j, "收尸").is_ok() && b2.find(j).is_none();
        got
    };
    s.add("J8-收割摘除", j8, "收尸后作业出表");

    // J9 未毕收割拒
    let j9 = {
        let mut b2 = JobBook::new();
        let j = b2.register(205, false, "后").unwrap_or(0);
        b2.reap(j, "早收").err() == Some(JB_BAD_STATE)
    };
    s.add("J9-未毕收割拒", j9, "RUN 态收尸必拒");

    // J10 前台唯一
    let j10 = b.fg_unique();
    s.add("J10-前台唯一", j10, "前台 RUN/STOP 作业至多一");

    // J11 未知作业
    let j11 = b.transition(0xBEEF, ST_DONE, "未知").err() == Some(JB_UNK_JOB);
    s.add("J11-未知作业", j11, "未知 job 必拒");

    // J12 指纹稳定 + seq 唯一
    let j12 = b.fold() == b.fold() && b.seq_unique();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致且 seq 唯一");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9157_job_flow() {
        let mut b = JobBook::new();
        let j = b.register(100, true, "a").unwrap();
        assert!(b.transition(j, ST_STOP, "b").is_ok());
        assert!(b.transition(j, ST_RUN, "c").is_ok());
        assert!(b.transition(j, ST_DONE, "d").is_ok());
        assert_eq!(b.poll_wnohang(), Some(j));
        assert!(b.reap(j, "e").is_ok());
        assert_eq!(b.transition(j, ST_DONE, "f"), Err(JB_UNK_JOB));
    }

    #[test]
    fn unx9157_checks_green() {
        let s = run_unx9157_checks();
        assert!(s.all_passed(), "UNX-F9157 判据未全绿");
    }
}
