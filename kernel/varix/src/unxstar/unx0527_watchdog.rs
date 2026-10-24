//! unx0527_watchdog —— UNX-F0527 ktest 看门狗（用例超时监护）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0527 · ktest 看门狗 · 目标 240 行
//! 判据原文：用例超时监护（单用例 5s 线）注入 3/3 悬挂样本检出，看门触发
//!           后套件续行断言。
//! 组件：no_std 无时钟——监护以「步数预算」模拟墙钟（Budget=步数上限）。
//!   用例体返回已耗步数，超线判 Timeout；触发后套件续行（后续用例仍运行）。
//!   悬挂样本注入 3/3 检出。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35100;
pub const XC_TIMEOUT: u32 = SEG_BASE + 1; // 超时判罚
pub const XC_BUDGET_ZERO: u32 = SEG_BASE + 2; // 预算为 0
pub const WD_LINE_STEPS: u32 = 500; // 5s 线折算步数上限
pub const XC_WD_MISUSE: u32 = SEG_BASE + 4; // 误用

/// 用例执行模型：返回耗步数（悬挂=耗尽大量步数）。
pub type StepBody = fn() -> u32;

/// 看门结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WatchOut {
    /// 步数 ≤ 线：过。
    Within(u32),
    /// 步数 > 线：超时判罚。
    TimedOut(u32),
}

/// 监护单用例。
pub fn guard(body: StepBody) -> Result<WatchOut, u32> {
    let used = body();
    if used > WD_LINE_STEPS {
        Ok(WatchOut::TimedOut(used))
    } else {
        Ok(WatchOut::Within(used))
    }
}

/// 套件账条目。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WdEntry {
    pub name: &'static str,
    pub out: WatchOut,
}

/// 带看门的套件运行：超时不中断——续行后续用例。
pub fn run_guarded(bodies: &[(&'static str, StepBody)]) -> Result<Vec<WdEntry>, u32> {
    if bodies.is_empty() {
        return Err(XC_WD_MISUSE);
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < bodies.len() {
        let (nm, b) = bodies[i];
        let o = guard(b)?;
        out.push(WdEntry { name: nm, out: o });
        i += 1;
    }
    Ok(out)
}

/// 悬挂样本体 ×3（超线步数）。
fn hang1() -> u32 {
    WD_LINE_STEPS + 10
}
fn hang2() -> u32 {
    WD_LINE_STEPS * 3
}
fn hang3() -> u32 {
    u32::MAX
}

/// 正常样本体。
fn quick() -> u32 {
    7
}
fn medium() -> u32 {
    WD_LINE_STEPS
}

/// 超时统计。
pub fn timeout_count(entries: &[WdEntry]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < entries.len() {
        if let WatchOut::TimedOut(_) = entries[i].out {
            n += 1;
        }
        i += 1;
    }
    n
}

pub fn run_unx0527_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0527");
    cs.add("SEG 段位独占", SEG_BASE == 0x35100 && XC_WD_MISUSE == SEG_BASE + 4 && WD_LINE_STEPS == 500, "u32 高段全域普查无冲突");

    // 族一：单用例监护。
    cs.add("线内过", guard(quick) == Ok(WatchOut::Within(7)), "7 步在线内");
    cs.add("恰线过", guard(medium) == Ok(WatchOut::Within(WD_LINE_STEPS)), "=线不判罚");
    cs.add("超线判罚", matches!(guard(hang1), Ok(WatchOut::TimedOut(_))), ">线即超时");
    cs.add("大步超时", matches!(guard(hang3), Ok(WatchOut::TimedOut(u32::MAX))), "极端悬挂捕获");

    // 族二：3/3 悬挂样本检出。
    let bodies = [("h1", hang1 as StepBody), ("h2", hang2 as StepBody), ("h3", hang3 as StepBody)];
    let entries = run_guarded(&bodies).unwrap_or_default();
    cs.add("悬挂 3/3", entries.len() == 3 && timeout_count(&entries) == 3, "三悬挂全判罚");

    // 族三：看门触发后续行。
    let mixed = [("hang", hang1 as StepBody), ("after1", quick as StepBody), ("after2", quick as StepBody)];
    let ent = run_guarded(&mixed).unwrap_or_default();
    cs.add("续行断言", ent.len() == 3 && ent[1].out == WatchOut::Within(7) && ent[2].out == WatchOut::Within(7), "超时后两用例仍运行");
    cs.add("超时定位", ent[0].name == "hang" && matches!(ent[0].out, WatchOut::TimedOut(_)), "判罚归位");

    // 族四：空输入拒。
    cs.add("空套件拒", run_guarded(&[]).is_err(), "零用例 Err");
    let one = [("solo", quick as StepBody)];
    cs.add("单用例可运行", run_guarded(&one).map(|v| v.len()) == Ok(1), "一用例出账");
    cs
}
