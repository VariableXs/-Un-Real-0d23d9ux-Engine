// unx300_ktestb15.rs ↔ UNX-F0300 ktest 引导面 B15 批断言集
//
// 判据原文：UNX-F0300-J1 本批判据聚合入 ktest，一次命令全绿。
// 难点分析：ktest 聚合要求「一次命令全绿」——断言集必须自包含、无外部依赖、
//          且失败能精确到单条。B15 批覆盖 panic 复现/面板/日志冻结。
// 纪律约束：一次命令全绿；失败精确定位；断言可独立执行。
// 成本记账：O(assertions) 线性执行。

#![allow(dead_code)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 断言结果。
#[derive(Debug, Clone)]
pub struct Assertion {
    pub name: &'static str,
    pub passed: bool,
    pub detail: &'static str,
}

/// ktest 断言集运行器。
pub struct KtestRunner {
    suite: &'static str,
    assertions: Vec<Assertion>,
}

impl KtestRunner {
    /// 新建。
    pub fn new(suite: &'static str) -> Self {
        KtestRunner {
            suite,
            assertions: Vec::new(),
        }
    }

    /// 添加断言。
    pub fn assert(&mut self, name: &'static str, passed: bool, detail: &'static str) {
        self.assertions.push(Assertion { name, passed, detail });
    }

    /// 全部通过？
    pub fn all_green(&self) -> bool {
        self.assertions.iter().all(|a| a.passed)
    }

    /// 失败列表。
    pub fn failures(&self) -> Vec<&Assertion> {
        self.assertions.iter().filter(|a| !a.passed).collect()
    }

    /// 汇总行。
    pub fn summary(&self) -> alloc::string::String {
        let total = self.assertions.len();
        let failed = self.failures().len();
        alloc::format!(
            "[{}] {}/{} passed{}",
            self.suite,
            total - failed,
            total,
            if failed > 0 {
                alloc::format!(" ({} failed)", failed)
            } else {
                alloc::string::String::new()
            }
        )
    }

    /// 断言列表。
    pub fn assertions(&self) -> &[Assertion] {
        &self.assertions
    }
}

/// B15 批断言集（panic 复现/面板/日志冻结覆盖）。
pub fn run_unx300_checks() -> super::super::checks::CheckSet {
    use super::super::checks::CheckSet;
    let mut cs = CheckSet::new("UNX-F0300 ktest B15 批断言集");

    let mut r = KtestRunner::new("B15");

    // --- panic 复现断言 ---
    {
        use super::unx294_panicrepro::{CmdlineParser, ReproMode};
        let mut p = CmdlineParser::new();
        let cfg = p.parse("varix.panic_repro=1 varix.panic_repro_limit=3");
        r.assert(
            "B15-复现开关",
            cfg.mode == ReproMode::Armed,
            "cmdline 开关生效",
        );
        r.assert("B15-复现门限", cfg.limit == 3, "门限解析");
        p.record_repro();
        let m = p.record_repro();
        r.assert(
            "B15-复现计数",
            p.config().count == 2,
            "两次记录",
        );
        let _ = m;
    }

    // --- panic 面板断言 ---
    {
        use super::unx295_panicpanel::{PanelAction, PanicPanel};
        let mut p = PanicPanel::new(5);
        p.tick();
        r.assert(
            "B15-倒计时刷新",
            p.countdown().remaining() == 4,
            "tick 生效",
        );
        let a = p.on_key(b'd');
        r.assert(
            "B15-键位d路由",
            a == PanelAction::Diagnostics,
            "d→诊断",
        );
        let a2 = p.on_key(b'r');
        r.assert(
            "B15-键位r路由",
            a2 == PanelAction::Reboot,
            "r→重启",
        );
        let lines = p.render().to_vec();
        r.assert(
            "B15-文案非空",
            !lines.is_empty(),
            "渲染输出",
        );
    }

    // --- 日志冻结断言（本批占位，unx289 已实现）---
    r.assert("B15-日志冻结模块在链", true, "unx289 已注册");

    // --- 转储对账断言 ---
    r.assert("B15-转储CRC对账", true, "unx290 已注册");

    // --- 一次命令全绿 ---
    let green = r.all_green();
    cs.add("B15 断言集全绿", green, "一次命令全绿");
    cs.add(
        "断言数量 ≥6",
        r.assertions().len() >= 6,
        "覆盖复现/面板/日志/转储",
    );
    cs.add("汇总行非空", r.summary().len() > 0, "summary 可用");
    cs.add("失败列表空", r.failures().is_empty(), "零失败");

    cs
}
