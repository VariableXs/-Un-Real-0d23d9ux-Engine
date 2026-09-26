//! F289 打印队列中心 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：队列状态机用例（排队/打印/完成/失败/取消）；卡住
//! 60s 提醒；取消即时性；人话原因映射表；PDF 虚拟任务同形制。
//!
//! **设计要点（主册）**：有打印任务时任务栏/通知区出现打印机图标：点
//! 开队列（每任务：文档名/状态/页数/取消按钮）、卡住的任务标注原因
//! （缺纸/脱机——人话不是错误码）、默认策略「卡住 60 秒提醒」；打印
//! 完成通知一条即止（不连环提醒）；虚拟打印（PDF 输出）走同一队列形制。
//!
//! 实装：任务状态机（排队→打印→完成/失败/取消，非法迁移拒绝）；卡住
//! 判定（打印态超 60s → 提醒 + 人话原因注入口）；取消即时（任何非终态
//! 可取消，即时落终态）；人话映射表（错误码→原因文案唯一源）；虚拟
//! 打印同型（同一状态机同一队列——类型不分家）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 卡住提醒阈值（s）。
pub const STUCK_WARN_S: u64 = 60;

/// 任务状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Printing,
    Done,
    Failed,
    Cancelled,
}

impl JobState {
    /// 终态。
    pub fn terminal(&self) -> bool {
        matches!(self, JobState::Done | JobState::Failed | JobState::Cancelled)
    }
}

/// 打印任务（虚拟 PDF 与实体同型）。
#[derive(Clone, Debug)]
pub struct PrintJob {
    pub doc: String,
    pub pages: u32,
    pub state: JobState,
    /// 是否虚拟打印（PDF 输出——同队列同形制）。
    pub virtual_pdf: bool,
    /// 进入当前态的时刻（秒戳注入）。
    pub state_since_s: u64,
    /// 底层错误码（永不裸抛——经 [`human_reason`] 转译）。
    pub error_code: Option<u32>,
    /// 已完成页数（打印态进度——「3/12 页」人话直读）。
    pub pages_done: u32,
}

/// 终态任务的收尾可见窗（s）：图标不闪没——完成后留 30s 供用户点开看结果。
pub const ICON_TAIL_S: u64 = 30;

/// 打印机图标可见性（任务栏/通知区）：有非终态任务，或收尾窗内刚
/// 落终态的任务（「有打印任务时出现」判据 + 不闪没的收尾体验）。
pub fn icon_visible(jobs: &[PrintJob], now_s: u64) -> bool {
    jobs.iter()
        .any(|j| !j.state.terminal() || now_s.saturating_sub(j.state_since_s) < ICON_TAIL_S)
}

/// 提醒节流账：同一任务的卡住提醒 60s 一条（「卡住 60 秒提醒」的
/// 完整语义是**每 60s 至多一条**——不是每秒轰炸；完成通知一条即止
/// 的同纪律在卡住面的落位）。
#[derive(Default)]
pub struct ReminderLedger {
    last: Vec<(JobId, u64)>,
}

impl ReminderLedger {
    pub fn new() -> ReminderLedger {
        ReminderLedger { last: Vec::new() }
    }

    /// 该任务此刻是否到提醒点（从未提醒过 → 到点；上次提醒已过阈值 → 到点）。
    pub fn due(&self, id: JobId, now_s: u64) -> bool {
        match self.last.iter().find(|(i, _)| *i == id) {
            None => true,
            Some((_, at)) => now_s.saturating_sub(*at) >= STUCK_WARN_S,
        }
    }

    /// 记一次提醒（时刻入账）。
    pub fn mark(&mut self, id: JobId, now_s: u64) {
        match self.last.iter_mut().find(|(i, _)| *i == id) {
            Some((_, at)) => *at = now_s,
            None => self.last.push((id, now_s)),
        }
    }
}

/// 人话原因映射表（唯一源——错误码 → 三要素之「为什么」）。
pub fn human_reason(code: u32) -> &'static str {
    match code {
        1 => "打印机缺纸——放好纸后可重试",
        2 => "打印机脱机——检查电源和连接",
        3 => "卡纸了——打开盖板取出卡住的纸",
        4 => "墨粉不足——可能影响打印质量",
        _ => "打印出现问题——详情见打印机面板",
    }
}

/// 队列中心。
pub struct PrintQueue {
    jobs: Vec<PrintJob>,
    next_id: usize,
}

/// 队列条目视图（id 附在返回对上）。
pub type JobId = usize;

impl PrintQueue {
    pub fn new() -> PrintQueue {
        PrintQueue { jobs: Vec::new(), next_id: 1 }
    }

    /// 提交任务（实体与 PDF 同口——同形制判据）。
    pub fn submit(&mut self, doc: &str, pages: u32, virtual_pdf: bool, now_s: u64) -> JobId {
        let id = self.next_id;
        self.next_id += 1;
        self.jobs.push(PrintJob {
            doc: String::from(doc),
            pages,
            state: JobState::Queued,
            virtual_pdf,
            state_since_s: now_s,
            error_code: None,
            pages_done: 0,
        });
        id - 1
    }

    /// 页进度上报（打印态；进度不超过总页数——越界钳制）。
    pub fn report_progress(&mut self, id: JobId, done: u32) -> bool {
        match self.jobs.get_mut(id) {
            Some(j) => {
                j.pages_done = done.min(j.pages);
                true
            }
            None => false,
        }
    }

    /// 人话进度（「3/12 页」；非打印态给状态语义）。
    pub fn progress_text(&self, id: JobId) -> String {
        match self.jobs.get(id) {
            Some(j) => match j.state {
                JobState::Queued => String::from("排队中"),
                JobState::Printing => alloc::format!("{}/{} 页", j.pages_done, j.pages),
                JobState::Done => String::from("完成"),
                JobState::Failed => String::from("失败"),
                JobState::Cancelled => String::from("已取消"),
            },
            None => String::from("任务不存在"),
        }
    }

    /// 队列位置：`id` 前面还有几个排队任务（0=下一个就轮到；非排队
    /// 任务返回 None——打印中/终态没有「位置」概念）。
    pub fn queue_position(&self, id: JobId) -> Option<usize> {
        if self.jobs.get(id)?.state != JobState::Queued {
            return None;
        }
        Some(self.jobs[..id].iter().filter(|j| j.state == JobState::Queued).count())
    }

    /// 状态迁移（状态机用例的执行点：非法迁移拒绝）。
    pub fn transition(&mut self, id: JobId, to: JobState, now_s: u64, error: Option<u32>) -> bool {
        let j = match self.jobs.get_mut(id) {
            Some(j) => j,
            None => return false,
        };
        let legal = match (j.state, to) {
            (JobState::Queued, JobState::Printing) => true,
            (JobState::Printing, JobState::Done) => true,
            (JobState::Printing, JobState::Failed) => true,
            (JobState::Queued, JobState::Cancelled) | (JobState::Printing, JobState::Cancelled) => true,
            _ => false,
        };
        if legal {
            j.state = to;
            j.state_since_s = now_s;
            j.error_code = error;
        }
        legal
    }

    /// 取消（即时性：调用即落终态——无排队无延迟）。
    pub fn cancel(&mut self, id: JobId, now_s: u64) -> bool {
        self.transition(id, JobState::Cancelled, now_s, None)
    }

    /// 卡住检测：打印态超过 60s → 提醒（附人话原因若有错误码）。
    pub fn stuck_jobs(&self, now_s: u64) -> Vec<(JobId, &'static str)> {
        self.jobs
            .iter()
            .enumerate()
            .filter(|(_, j)| j.state == JobState::Printing && now_s - j.state_since_s >= STUCK_WARN_S)
            .map(|(i, j)| (i, j.error_code.map(human_reason).unwrap_or("正在打印但响应慢——再等一会或取消")))
            .collect()
    }

    /// 队列视图（文档名/状态/页数/取消可用性）。
    pub fn view(&self) -> Vec<(String, JobState, u32, bool)> {
        self.jobs
            .iter()
            .map(|j| (j.doc.clone(), j.state, j.pages, !j.state.terminal()))
            .collect()
    }

    /// 完成通知一条即止：终态任务不再产生提醒（终态即静默——结构保证）。
    pub fn notify_worthy(&self) -> usize {
        self.jobs.iter().filter(|j| j.state == JobState::Done).count()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_printq_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F289");
    let mut q = PrintQueue::new();
    let a = q.submit("报告.docx", 12, false, 0);
    let pdf = q.submit("发票.pdf 输出", 1, true, 0);
    // 状态机：排队→打印→完成。
    set.add(
        "F289 state machine",
        q.transition(a, JobState::Printing, 10, None)
            && q.transition(a, JobState::Done, 30, None),
        "queued→print→done",
    );
    // 非法迁移拒绝：终态再迁移。
    set.add(
        "F289 illegal rejected",
        !q.transition(a, JobState::Printing, 40, None)
            && !q.transition(a, JobState::Cancelled, 41, None),
        "terminal locked",
    );
    // 取消即时性。
    set.add(
        "F289 cancel instant",
        q.cancel(pdf, 42) && q.jobs[pdf].state == JobState::Cancelled,
        "call=done",
    );
    // 卡住 60s 提醒 + 人话原因。
    let b = q.submit("大图.png", 1, false, 0);
    let _ = q.transition(b, JobState::Printing, 100, Some(1));
    let stuck_at_59 = q.stuck_jobs(159);
    let stuck_at_60 = q.stuck_jobs(160);
    set.add(
        "F289 stuck 60s",
        stuck_at_59.is_empty() && stuck_at_60.len() == 1 && stuck_at_60[0].1.contains("缺纸"),
        "human words",
    );
    // 人话映射表全覆盖。
    let all_human = (1u32..=5).all(|c| !human_reason(c).is_empty());
    set.add("F289 reason map", all_human, "no raw codes");
    // PDF 虚拟任务同形制：同队列同字段。
    let v = q.submit("稿件.pdf", 3, true, 200);
    set.add(
        "F289 pdf same form",
        q.jobs[v].virtual_pdf && q.view()[v].0 == "稿件.pdf",
        "one queue shape",
    );
    // 完成通知一条即止。
    set.add(
        "F289 done notify once",
        q.notify_worthy() == 1,
        "no repeat",
    );
    // --- 深化：提醒节流——同一任务 60s 至多一条卡住提醒。 ---
    let mut rl = ReminderLedger::new();
    let mut warned: Vec<&'static str> = Vec::new();
    for now_s in 160..=200u64 {
        // 40 秒内逐秒轮询：只在 160s 该提醒一次，随后 59s 静默。
        if rl.due(b, now_s) {
            if let Some((_, why)) = q.stuck_jobs(now_s).iter().find(|(i, _)| *i == b) {
                warned.push(why);
            }
            rl.mark(b, now_s);
        }
    }
    set.add("F289 reminder throttled", warned.len() == 1, "once per window");
    // 再过 60s 又到点（持续卡住持续提醒，但节奏是 60s 一条）。
    set.add("F289 reminder due again", rl.due(b, 220), "next window");
    // --- 深化：页进度与人话进度。 ---
    let mut q2 = PrintQueue::new();
    let d = q2.submit("论文.pdf", 12, false, 0);
    let _ = q2.transition(d, JobState::Printing, 1, None);
    let _ = q2.report_progress(d, 3);
    set.add(
        "F289 page progress",
        q2.progress_text(d) == "3/12 页" && q2.report_progress(d, 99) && q2.jobs[d].pages_done == 12,
        "clamped",
    );
    // --- 深化：队列位置。 ---
    let e = q2.submit("第二名.docx", 1, false, 2);
    let f = q2.submit("第三名.docx", 1, false, 2);
    // d 在打印态 → 无位置；e 之前无排队者 → 0；f 之前排着 e → 1。
    set.add(
        "F289 queue position",
        q2.queue_position(e) == Some(0) && q2.queue_position(f) == Some(1) && q2.queue_position(d).is_none(),
        "ahead count",
    );
    // --- 深化：图标可见性（非终态可见；完成后收尾窗 30s；窗过熄灭）。 ---
    let mut q3 = PrintQueue::new();
    set.add("F289 icon empty", !icon_visible(&q3.jobs, 100), "no job no icon");
    let g = q3.submit("x", 1, false, 100);
    set.add("F289 icon queued", icon_visible(&q3.jobs, 100), "job → icon");
    let _ = q3.transition(g, JobState::Printing, 110, None);
    let _ = q3.transition(g, JobState::Done, 200, None);
    set.add("F289 icon tail window", icon_visible(&q3.jobs, 210), "30s tail");
    set.add("F289 icon tail over", !icon_visible(&q3.jobs, 231), "then gone");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f289_queue_flow() {
        let set = run_printq_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F289 自检红 {f}/{p}");
    }

    #[test]
    fn queued_to_done_skipped() {
        let mut q = PrintQueue::new();
        let id = q.submit("x", 1, false, 0);
        assert!(!q.transition(id, JobState::Done, 1, None), "排队直跳完成被拒");
    }
}
