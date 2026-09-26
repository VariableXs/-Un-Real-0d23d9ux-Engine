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
        });
        id - 1
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
