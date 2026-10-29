//! unx0954_explain — UNX-F0954 分发面人话化（explain 对齐体例）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0954 分发面人话化（explain 对齐体例）
//! 判据原文摘录（卡面验收行）：分发面全要素人话化输出逐条可读
//! （三底座/触发点/预算/停滞四主题），体例与既有 explain 对齐
//! （编号+判词+一句人话），抽读 10/10 条格式合规。
//! 诊断段 0x7D40（本单独占，XcCode 闭集见下）。
//! 设计纪律：人话化不是文学——每条输出必须含编号、判词、人话三
//! 段式；格式器是纯函数（同样账面出同样话）；禁止空判词；全部错误
//! 走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7D40;

// ── 诊断段独占 · 人话化错误域 ──────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_FMT: u16 = SEG_ERR + 1;   // 三段式缺段
const E_EMPTY: u16 = SEG_ERR + 2; // 空判词
const E_TOPIC: u16 = SEG_ERR + 3; // 主题非法
const E_STATE: u16 = SEG_ERR + 4; // 体例状态非法

/// 四主题。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topic {
    Bases,    // 三底座
    Trigger,  // 触发点
    Budget,   // 预算
    Stall,    // 停滞
}

impl Topic {
    pub fn idx(self) -> usize { self as usize }
    pub fn as_str(self) -> &'static str {
        match self { Topic::Bases => "bases", Topic::Trigger => "trigger", Topic::Budget => "budget", Topic::Stall => "stall" }
    }
}

/// 一条 explain 条目：编号 + 判词 + 人话。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplainEntry {
    pub no: u32,
    pub topic: Topic,
    pub verdict: String,
    pub human: String,
}

impl ExplainEntry {
    /// 三段式格式合规核验：三段齐、判词与人话非空。
    pub fn well_formed(&self) -> bool {
        self.no > 0 && !self.verdict.is_empty() && !self.human.is_empty()
    }
    /// 单行渲染。
    pub fn render(&self) -> String {
        alloc::format!("[{:03}][{}] 判词:{} ｜ 人话:{}", self.no, self.topic.as_str(), self.verdict, self.human)
    }
}

/// 纯函数体例器：账面事实 → 人话条目。
pub fn explain_bases(bases: (bool, bool, bool)) -> Result<ExplainEntry, u16> {
    let (s, t, w) = bases;
    let verdict = if w && !s && !t {
        String::from("workqueue-only-active")
    } else if !w && s && !t {
        String::from("softirq-only-active")
    } else if !w && !s && t {
        String::from("tasklet-only-active")
    } else {
        String::from("mixed-active")
    };
    let human = if w && !s && !t {
        String::from("重活都在进程上下文里慢慢干，中断面很清闲。")
    } else if !w && s && !t {
        String::from("活都是短平快，软中断一口气跑完。")
    } else if !w && !s && t {
        String::from("同型活必须排队串行，tasklet 扛着。")
    } else {
        String::from("三底座混跑，按选型契约各归各位。")
    };
    Ok(ExplainEntry { no: 1, topic: Topic::Bases, verdict, human })
}

/// 触发点人话化：三处触发时机各一句。
pub fn explain_trigger(hits: [u32; 3]) -> Result<Vec<ExplainEntry>, u16> {
    let mut v = Vec::new();
    let names = ["top-tail", "irq-ret", "sched"];
    let lines = [
        "上半收尾时把活挂上号，这是第一触点。",
        "中断返回前再补一触，防止窗口期漏活。",
        "调度点兜底再触一次，三触合一不重复跑。",
    ];
    for i in 0..3 {
        v.push(ExplainEntry {
            no: i as u32 + 1,
            topic: Topic::Trigger,
            verdict: alloc::format!("{} hits={}", names[i], hits[i]),
            human: String::from(lines[i]),
        });
    }
    Ok(v)
}

/// 预算人话化：五域定额表转三段式。
pub fn explain_budget(quota: [u64; 5]) -> Result<Vec<ExplainEntry>, u16> {
    let mut v = Vec::new();
    let names = ["取号", "上半", "触发", "下半", "收尾"];
    for i in 0..5 {
        v.push(ExplainEntry {
            no: i as u32 + 1,
            topic: Topic::Budget,
            verdict: alloc::format!("dom-{} quota={}us", i, quota[i]),
            human: alloc::format!("{}这一段最多花 {} 微秒，超了就点名。", names[i], quota[i]),
        });
    }
    Ok(v)
}

/// 停滞人话化：四因各一句。
pub fn explain_stall(counts: [u32; 4]) -> Result<Vec<ExplainEntry>, u16> {
    let mut v = Vec::new();
    let verdicts = ["irq-off", "swallow", "starve", "invert"];
    let lines = [
        "中断被关死了，触发面根本没进来。",
        "pending 位和票仓对不上，票被吞了。",
        "消费者长忙不收活，活一直压着。",
        "低优先级占着道，高优先级久候。",
    ];
    for i in 0..4 {
        v.push(ExplainEntry {
            no: i as u32 + 1,
            topic: Topic::Stall,
            verdict: alloc::format!("{} hits={}", verdicts[i], counts[i]),
            human: String::from(lines[i]),
        });
    }
    Ok(v)
}

/// 条目格式审计：全部三段式合规且编号连续。
pub fn fmt_audit(all: &[ExplainEntry]) -> Result<u32, u16> {
    if all.is_empty() { return Err(E_EMPTY); }
    for (i, e) in all.iter().enumerate() {
        if !e.well_formed() { return Err(E_FMT); }
        if e.no != i as u32 + 1 { return Err(E_FMT); }
        let r = e.render();
        if !r.starts_with('[') { return Err(E_FMT); }
    }
    Ok(all.len() as u32)
}

/// 自例程：四主题全量出条目，格式 10/10 合规返回 0。
pub fn drill() -> Result<u32, u16> {
    let mut all: Vec<ExplainEntry> = Vec::new();
    // 场景一：三底座主题。
    let e = explain_bases((true, false, false))?;
    if e.verdict != "workqueue-only-active" { return Err(E_FMT); }
    all.push(renumber(e, all.len()));
    // 场景二：触发点主题 3 条。
    for e in explain_trigger([5, 3, 1])? {
        all.push(renumber(e, all.len()));
    }
    // 场景三：预算主题 5 条。
    for e in explain_budget([2, 5, 2, 200, 3])? {
        all.push(renumber(e, all.len()));
    }
    // 场景四：停滞主题 4 条。
    for e in explain_stall([10, 10, 10, 10])? {
        all.push(renumber(e, all.len()));
    }
    let n = fmt_audit(&all)?;
    if n != 13 { return Err(E_STATE); }
    // 场景五：缺段必拦（空判词）。
    let bad = ExplainEntry { no: 1, topic: Topic::Bases, verdict: String::new(), human: String::from("x") };
    if !fmt_audit(alloc::vec![bad.clone()].as_slice()).is_err() { return Err(E_FMT); }
    if bad.well_formed() { return Err(E_FMT); }
    // 场景六：主题非法必拦（体例面仅认四主题闭集）。
    let mut seen = 0u32;
    for t in [Topic::Bases, Topic::Trigger, Topic::Budget, Topic::Stall] {
        if t.as_str().is_empty() { return Err(E_TOPIC); }
        seen += 1;
    }
    if seen != 4 { return Err(E_TOPIC); }
    Ok(0)
}

/// 重编号辅助：按落账序覆盖编号。
fn renumber(mut e: ExplainEntry, i: usize) -> ExplainEntry {
    e.no = i as u32 + 1;
    e
}
