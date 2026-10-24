//! VE-F4346 · 一致性回归体系（U08 测试与资产组）
//! 职责：一致性回归测试体系——回归三线（规则变更回归 / 引擎变更回归 /
//! 工具面变更回归）+ 回归编排（按影响面选线——复用影响分析）+ 回归结果
//! 处置（失败即阻断 + 账本立案）+ 选线失准全量兜底重跑 + 编排死锁超时
//! 放行告警 + 阻断滥用分级放行评审 + 无障碍基线线（域本色）。
//! 段位：0x4060（续表 idx580）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x4060;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RgCode {
    /// 回归失败（阻断 + 立案）。
    RegressBlocked = SEG_BASE,
    /// 选线失准（全量兜底重跑）。
    LineMisPicked = SEG_BASE + 0x01,
    /// 编排死锁（超时放行 + 告警）。
    Deadlock = SEG_BASE + 0x02,
    /// 阻断滥用（分级放行评审）。
    BlockAbuse = SEG_BASE + 0x03,
}

/// 回归三线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Line {
    RuleChange,
    EngineChange,
    ToolChange,
}

/// 影响面描述（选线输入）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Impact {
    /// 规则层受影响。
    pub rule: bool,
    /// 引擎层受影响。
    pub engine: bool,
    /// 工具面受影响。
    pub tool: bool,
}

/// 按影响面选线（可多线并发；零影响 = 全量兜底信号）。
pub fn pick_lines(i: Impact) -> Vec<Line> {
    let mut v = Vec::new();
    if i.rule {
        v.push(Line::RuleChange);
    }
    if i.engine {
        v.push(Line::EngineChange);
    }
    if i.tool {
        v.push(Line::ToolChange);
    }
    v
}

/// 编排任务。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orchestrate {
    pub line: Line,
    /// 已等待 tick。
    pub waited: u32,
    pub passed: Option<bool>,
}

/// 回归台。
#[derive(Default)]
pub struct RegressDesk {
    pub queue: Vec<Orchestrate>,
    /// 超时阈（tick）。
    pub timeout: u32,
    pub cases: Vec<(Line, bool)>,
    pub blocks: u32,
    pub ledgers: u32,
    pub alerts: u32,
    pub full_reruns: u32,
    pub reviews: u32,
}

/// 死锁超时阈。
pub const DEADLOCK_TICKS: u32 = 300;

impl RegressDesk {
    pub fn new(timeout: u32) -> Self {
        RegressDesk { timeout: timeout.max(1), ..Default::default() }
    }

    /// 编排入队（按选线结果）。
    pub fn enqueue(&mut self, lines: &[Line]) {
        for l in lines {
            self.queue.push(Orchestrate { line: *l, waited: 0, passed: None });
        }
    }

    /// 时钟推进（等待计满超时 → 放行 + 告警）。
    pub fn tick(&mut self, n: u32) -> usize {
        let mut released = 0;
        for o in self.queue.iter_mut() {
            o.waited += n;
            if o.waited >= self.timeout && o.passed.is_none() {
                o.passed = Some(true); // 超时放行
                self.alerts += 1;
                released += 1;
            }
        }
        released
    }

    /// 回归执行：失败即阻断 + 账本立案。
    pub fn finish(&mut self, line: Line, passed: bool) -> Result<(), RgCode> {
        if let Some(o) = self.queue.iter_mut().find(|o| o.line == line && o.passed.is_none()) {
            o.passed = Some(passed);
        }
        self.cases.push((line, passed));
        if !passed {
            self.blocks += 1;
            self.ledgers += 1;
            return Err(RgCode::RegressBlocked);
        }
        Ok(())
    }

    /// 选线失准兜底：执行结果发现未选线也失败 → 全量重跑（计数）。
    pub fn mispick_fallback(&mut self, unexpected: Line) -> usize {
        let picked = self.queue.iter().any(|o| o.line == unexpected);
        if !picked {
            self.full_reruns += 1;
            self.enqueue(&[Line::RuleChange, Line::EngineChange, Line::ToolChange]);
            return 3;
        }
        0
    }

    /// 阻断滥用评审：阻断数超阈（>10/轮）→ 触发分级放行评审（返回评审放行数）。
    pub fn abuse_review(&mut self, threshold: u32) -> usize {
        if self.blocks > threshold {
            let grant = self.blocks - threshold;
            self.blocks = threshold;
            self.reviews += 1;
            grant as usize
        } else {
            0
        }
    }

    /// 无障碍基线线断言（域色）：任一线执行须携带无障碍基线校验。
    pub fn a11y_baseline(&self, carried: bool) -> bool {
        !self.cases.is_empty() && carried
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves580_checks() -> CheckSet {
    let mut set = CheckSet::new("ves580");

    // C580-01 按影响面选线（三向独立）。
    let l1 = pick_lines(Impact { rule: true, engine: false, tool: false });
    let l2 = pick_lines(Impact { rule: false, engine: true, tool: true });
    chk(&mut set, "C580-01", l1 == [Line::RuleChange] && l2 == [Line::EngineChange, Line::ToolChange], "影响面选线");

    // C580-02 零影响 = 空线（全量兜底信号由 mispick 处理）。
    chk(&mut set, "C580-02", pick_lines(Impact { rule: false, engine: false, tool: false }).is_empty(), "零影响空线");

    // C580-03 编排入队 + 执行通过。
    let mut rd = RegressDesk::new(DEADLOCK_TICKS);
    rd.enqueue(&l2);
    chk(&mut set, "C580-03", rd.finish(Line::EngineChange, true).is_ok(), "回归通过");

    // C580-04 回归失败阻断 + 立案。
    chk(&mut set, "C580-04", rd.finish(Line::ToolChange, false) == Err(RgCode::RegressBlocked)
        && rd.blocks == 1 && rd.ledgers == 1, "失败阻断立案");

    // C580-05 选线失准全量兜底重跑。
    let n = rd.mispick_fallback(Line::RuleChange);
    chk(&mut set, "C580-05", n == 3 && rd.full_reruns == 1 && rd.queue.len() == 3, "失准全量兜底");

    // C580-06 已选线不误兜底。
    chk(&mut set, "C580-06", rd.mispick_fallback(Line::EngineChange) == 0, "已选线不兜底");

    // C580-07 编排死锁超时放行 + 告警。
    let mut rd2 = RegressDesk::new(100);
    rd2.enqueue(&[Line::RuleChange, Line::EngineChange]);
    let rel = rd2.tick(100);
    chk(&mut set, "C580-07", rel == 2 && rd2.alerts == 2, "超时放行告警");

    // C580-08 未到超时不放行。
    let mut rd3 = RegressDesk::new(100);
    rd3.enqueue(&[Line::RuleChange]);
    chk(&mut set, "C580-08", rd3.tick(99) == 0 && rd3.alerts == 0, "未到阈不放行");

    // C580-09 阻断滥用分级放行评审。
    let mut rd4 = RegressDesk::new(300);
    rd4.enqueue(&[Line::RuleChange, Line::EngineChange, Line::ToolChange]);
    let _ = rd4.finish(Line::RuleChange, false);
    let _ = rd4.finish(Line::EngineChange, false);
    let _ = rd4.finish(Line::ToolChange, false);
    let grant = rd4.abuse_review(1);
    chk(&mut set, "C580-09", grant == 2 && rd4.reviews == 1 && rd4.blocks == 1, "滥用评审放行 2");

    // C580-10 无滥用不评审。
    chk(&mut set, "C580-10", rd4.abuse_review(5) == 0 && rd4.reviews == 1, "无滥用不评审");

    // C580-11 无障碍基线线断言（域色）。
    chk(&mut set, "C580-11", rd4.a11y_baseline(true) && !rd4.a11y_baseline(false), "无障碍基线断言");

    // C580-12 空台基线断言恒假（不臆测）。
    let rd5 = RegressDesk::new(10);
    chk(&mut set, "C580-12", !rd5.a11y_baseline(true), "空台不臆测");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_by_impact() {
        assert_eq!(pick_lines(Impact { rule: true, engine: true, tool: false }).len(), 2);
        assert!(pick_lines(Impact { rule: false, engine: false, tool: false }).is_empty());
    }

    #[test]
    fn deadlock_release() {
        let mut rd = RegressDesk::new(50);
        rd.enqueue(&[Line::RuleChange]);
        assert_eq!(rd.tick(49), 0);
        assert_eq!(rd.tick(1), 1);
        assert_eq!(rd.alerts, 1);
        assert_eq!(rd.tick(100), 0); // 已放行不再告警
    }

    #[test]
    fn fail_blocks_and_ledgers() {
        let mut rd = RegressDesk::new(300);
        rd.enqueue(&[Line::ToolChange]);
        assert_eq!(rd.finish(Line::ToolChange, false), Err(RgCode::RegressBlocked));
        assert_eq!(rd.blocks, 1);
        assert_eq!(rd.ledgers, 1);
    }
}
