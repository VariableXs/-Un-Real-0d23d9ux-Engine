//! CGPU-F0389 · 预算报表引擎（目标 340 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 报表：周期报表（日报/周报：账户消耗 Top/违规统计/降档汇总/公平指数
//!   趋势）；模板（内置+自定义）；开放导出（JSON/CSV）；定时异步生成
//!   （不阻塞）。判据：双周期、模板、导出、异步、判据。
//!
//! 设计纪律：
//! - 导出是纯函数：账本快照 → 字符串，零副作用（幂等可回归）。
//! - 异步以任务队列呈现：入队/出队/状态机（Pending→Done），不阻塞主路径。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0B20 独占。

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;

/// 诊断段基址（0x0B20，全仓独占）。
pub const SEG_BASE: u16 = 0x0B20;

pub const E_PERIOD_UNKNOWN: u16 = SEG_BASE + 1;
pub const E_TEMPLATE_UNKNOWN: u16 = SEG_BASE + 2;
pub const E_EXPORT_EMPTY: u16 = SEG_BASE + 3;
pub const E_QUEUE_FULL: u16 = SEG_BASE + 4;

/// 队列容量。
pub const QUEUE_CAP: usize = 16;

/// 周期（判据「双周期」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Period {
    Daily,
    Weekly,
}

impl Period {
    pub fn tag(self) -> &'static str {
        match self {
            Period::Daily => "daily",
            Period::Weekly => "weekly",
        }
    }
}

/// 账户消耗行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpendRow {
    pub account: u32,
    pub spent_pm: u32,
    /// 违规次数。
    pub violations: u32,
    /// 降档次数。
    pub downgrades: u32,
}

/// 公平指数趋势点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FairPoint {
    pub frame: u64,
    pub fairness_pm: u32,
}

/// 周期账本快照（报表输入真源）。
#[derive(Clone, Debug, Default)]
pub struct PeriodLedger {
    pub period: Option<Period>,
    pub spends: Vec<SpendRow>,
    pub fairness_trend: Vec<FairPoint>,
}

impl PeriodLedger {
    pub fn open(&mut self, p: Period) {
        self.period = Some(p);
    }

    /// Top-N 消耗账户（降序；并列按账户号升序——确定性）。
    pub fn top_spend(&self, n: usize) -> Vec<SpendRow> {
        let mut sorted = self.spends.clone();
        sorted.sort_by(|a, b| b.spent_pm.cmp(&a.spent_pm).then(a.account.cmp(&b.account)));
        sorted.into_iter().take(n).collect()
    }

    /// 违规统计（有违规的账户数与总次数）。
    pub fn violation_summary(&self) -> (usize, u32) {
        let accounts = self.spends.iter().filter(|r| r.violations > 0).count();
        let total: u32 = self.spends.iter().map(|r| r.violations).sum();
        (accounts, total)
    }

    /// 降档汇总。
    pub fn downgrade_total(&self) -> u32 {
        self.spends.iter().map(|r| r.downgrades).sum()
    }

    /// 公平指数趋势：首末差（正=趋好）。
    pub fn fairness_trend_delta(&self) -> Option<i64> {
        let first = self.fairness_trend.first()?;
        let last = self.fairness_trend.last()?;
        Some(last.fairness_pm as i64 - first.fairness_pm as i64)
    }
}

/// 模板（内置 + 自定义——渲染段顺序表）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    pub name: String,
    /// 段序：T=Top 消耗 V=违规 D=降档 F=公平趋势。
    pub sections: Vec<u8>,
    pub builtin: bool,
}

pub fn builtin_templates() -> Vec<Template> {
    vec![
        Template { name: String::from("daily-standard"), sections: vec![b'T', b'V', b'D', b'F'], builtin: true },
        Template { name: String::from("weekly-brief"), sections: vec![b'T', b'F'], builtin: true },
    ]
}

/// JSON 导出（开放导出——纯函数）。
pub fn export_json(l: &PeriodLedger, t: &Template) -> Result<String, u16> {
    let p = l.period.ok_or(E_PERIOD_UNKNOWN)?;
    let mut out = String::from("{\"period\":\"");
    out.push_str(p.tag());
    out.push_str("\",\"sections\":");
    for (i, &sec) in t.sections.iter().enumerate() {
        match sec {
            b'T' => {
                let top = l.top_spend(3);
                out.push_str("\"top\":[");
                for (j, r) in top.iter().enumerate() {
                    if j > 0 {
                        out.push(',');
                    }
                    out.push_str(&format(r.account, r.spent_pm));
                }
                out.push(']');
            }
            b'V' => {
                let (acc, total) = l.violation_summary();
                out.push_str(&format_args_violations(acc, total));
            }
            b'D' => {
                out.push_str(&format_args_downgrades(l.downgrade_total()));
            }
            b'F' => {
                if let Some(d) = l.fairness_trend_delta() {
                    out.push_str(&format_args_fair(d));
                }
            }
            _ => return Err(E_TEMPLATE_UNKNOWN),
        }
        if i + 1 < t.sections.len() {
            out.push(',');
        }
    }
    out.push('}');
    Ok(out)
}

fn format(account: u32, spent: u32) -> String {
    let mut s = String::from("{\"account\":");
    s.push_str(&itoa(account));
    s.push_str(",\"spent_pm\":");
    s.push_str(&itoa(spent));
    s.push('}');
    s
}

fn itoa(mut v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    String::from(core::str::from_utf8(&buf[i..]).unwrap_or("0"))
}

fn format_args_violations(accounts: usize, total: u32) -> String {
    let mut s = String::from("\"violations\":{\"accounts\":");
    s.push_str(&itoa(accounts as u32));
    s.push_str(",\"total\":");
    s.push_str(&itoa(total));
    s.push('}');
    s
}

fn format_args_downgrades(total: u32) -> String {
    let mut s = String::from("\"downgrades\":");
    s.push_str(&itoa(total));
    s
}

fn format_args_fair(delta: i64) -> String {
    let mut s = String::from("\"fair_delta\":");
    if delta < 0 {
        s.push('-');
        s.push_str(&itoa((-delta) as u32));
    } else {
        s.push_str(&itoa(delta as u32));
    }
    s
}

/// CSV 导出（Top 消耗面——纯函数）。
pub fn export_csv(l: &PeriodLedger) -> Result<String, u16> {
    let p = l.period.ok_or(E_PERIOD_UNKNOWN)?;
    if l.spends.is_empty() {
        return Err(E_EXPORT_EMPTY);
    }
    let mut out = String::from("account,spent_pm,violations,downgrades\n");
    for r in l.top_spend(usize::MAX) {
        out.push_str(itoa(r.account).as_str());
        out.push(',');
        out.push_str(itoa(r.spent_pm).as_str());
        out.push(',');
        out.push_str(itoa(r.violations).as_str());
        out.push(',');
        out.push_str(itoa(r.downgrades).as_str());
        out.push('\n');
    }
    let _ = p;
    Ok(out)
}

/// 异步任务状态机（判据「定时异步生成」——不阻塞主路径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobState {
    Pending,
    Done,
}

pub struct ReportJob {
    pub period: Period,
    pub state: JobState,
    pub payload: Option<String>,
}

pub struct JobQueue {
    jobs: Vec<ReportJob>,
}

impl JobQueue {
    pub fn new() -> Self {
        JobQueue { jobs: Vec::new() }
    }

    /// 入队（不阻塞：只登记，不生成）。
    pub fn enqueue(&mut self, p: Period) -> Result<usize, u16> {
        if self.jobs.len() >= QUEUE_CAP {
            return Err(E_QUEUE_FULL);
        }
        self.jobs.push(ReportJob { period: p, state: JobState::Pending, payload: None });
        Ok(self.jobs.len() - 1)
    }

    /// 出队并生成（消费侧拉取——生产者永不被阻塞）。
    pub fn work_one(&mut self, idx: usize, l: &PeriodLedger, t: &Template) -> Result<(), u16> {
        if idx >= self.jobs.len() {
            return Err(E_PERIOD_UNKNOWN);
        }
        if self.jobs[idx].state == JobState::Done {
            return Ok(());
        }
        let payload = export_json(l, t)?;
        self.jobs[idx].state = JobState::Done;
        self.jobs[idx].payload = Some(payload);
        Ok(())
    }

    pub fn pending(&self) -> usize {
        self.jobs.iter().filter(|j| j.state == JobState::Pending).count()
    }

    pub fn done_payload(&self, idx: usize) -> Option<&str> {
        self.jobs.get(idx).and_then(|j| j.payload.as_deref())
    }
}

impl Default for JobQueue {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据自检
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn ledger() -> PeriodLedger {
        let mut l = PeriodLedger::default();
        l.open(Period::Daily);
        l.spends.push(SpendRow { account: 3, spent_pm: 900, violations: 2, downgrades: 1 });
        l.spends.push(SpendRow { account: 1, spent_pm: 700, violations: 0, downgrades: 0 });
        l.spends.push(SpendRow { account: 2, spent_pm: 900, violations: 1, downgrades: 0 });
        l.fairness_trend.push(FairPoint { frame: 1, fairness_pm: 600 });
        l.fairness_trend.push(FairPoint { frame: 2, fairness_pm: 650 });
        l
    }

    #[test]
    fn a89_双周期_Top与统计() {
        let mut l = ledger();
        assert_eq!(l.period, Some(Period::Daily));
        // Top3：并列 900 时账户号升序（2 在 3 前）。
        let top = l.top_spend(3);
        assert_eq!(top[0].account, 2);
        assert_eq!(top[1].account, 3);
        assert_eq!(top[2].account, 1);
        // 违规与降档汇总。
        assert_eq!(l.violation_summary(), (2, 3));
        assert_eq!(l.downgrade_total(), 1);
        // 趋势差：650−600=+50（趋好）。
        assert_eq!(l.fairness_trend_delta(), Some(50));
        // 未开账周期。
        let empty = PeriodLedger::default();
        assert_eq!(empty.top_spend(1).len(), 0);
        assert_eq!(export_json(&empty, &builtin_templates()[0]).unwrap_err(), E_PERIOD_UNKNOWN);
        // 周期标签。
        l.open(Period::Weekly);
        assert_eq!(l.period, Some(Period::Weekly));
        assert_eq!(Period::Weekly.tag(), "weekly");
    }

    #[test]
    fn a89_模板与JSON导出() {
        let l = ledger();
        let tpls = builtin_templates();
        assert_eq!(tpls.len(), 2);
        assert!(tpls.iter().all(|t| t.builtin));
        // daily-standard 含四段。
        let j = export_json(&l, &tpls[0]).unwrap();
        assert!(j.starts_with("{\"period\":\"daily\""));
        assert!(j.contains("\"top\":["));
        assert!(j.contains("\"account\":2,\"spent_pm\":900"));
        assert!(j.contains("\"violations\":{\"accounts\":2,\"total\":3}"));
        assert!(j.contains("\"downgrades\":1"));
        assert!(j.contains("\"fair_delta\":50"));
        // weekly-brief 只两段（T,F）。
        let j2 = export_json(&l, &tpls[1]).unwrap();
        assert!(!j2.contains("violations"));
        assert!(!j2.contains("downgrades"));
        // 自定义模板：未知段码拒。
        let bad = Template { name: String::from("x"), sections: vec![b'Z'], builtin: false };
        assert_eq!(export_json(&l, &bad).unwrap_err(), E_TEMPLATE_UNKNOWN);
    }

    #[test]
    fn a89_CSV导出与异步队列() {
        let l = ledger();
        let csv = export_csv(&l).unwrap();
        assert!(csv.starts_with("account,spent_pm,violations,downgrades\n"));
        assert!(csv.contains("2,900,1,0\n"));
        assert!(csv.contains("3,900,2,1\n"));
        // 空账（未开周期）导出拒：周期校验在先。
        assert_eq!(export_csv(&PeriodLedger::default()).unwrap_err(), E_PERIOD_UNKNOWN);

        // 异步：入队不生成（pending>0），work_one 后 Done。
        let mut q = JobQueue::new();
        let i1 = q.enqueue(Period::Daily).unwrap();
        let _i2 = q.enqueue(Period::Weekly).unwrap();
        assert_eq!(q.pending(), 2);
        assert!(q.done_payload(i1).is_none());
        q.work_one(i1, &l, &builtin_templates()[0]).unwrap();
        assert_eq!(q.pending(), 1);
        assert!(q.done_payload(i1).unwrap().starts_with("{\"period\":\"daily\""));
        // 重复 work 幂等。
        q.work_one(i1, &l, &builtin_templates()[0]).unwrap();
        assert_eq!(q.pending(), 1);
        // 越界 job 拒。
        assert_eq!(q.work_one(9, &l, &builtin_templates()[0]).unwrap_err(), E_PERIOD_UNKNOWN);
        // 队列满。
        let mut full = JobQueue::new();
        for _ in 0..QUEUE_CAP {
            full.enqueue(Period::Daily).unwrap();
        }
        assert_eq!(full.enqueue(Period::Daily).unwrap_err(), E_QUEUE_FULL);
    }
}
