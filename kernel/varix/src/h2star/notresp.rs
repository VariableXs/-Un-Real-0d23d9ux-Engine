//! F284 无响应判定与恢复 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：5s 判定阈值注入测试；蒙层视觉与标注；两选一默认
//! 不动；抢救快照恢复用例（记事本长文场景）；结束成功率。
//!
//! **设计要点（主册）**：应用主线程 5 秒无响应判定为挂起：窗口不白屏
//! 不假死（内容冻结在最后一帧加暗化 20% 蒙层），窗口标题栏出现「无
//! 响应」标注并弹两选一浮条（等待/结束）——默认不动（再等 5s 自动消
//! 失，可能只是慢）；「结束」走体面终止（保存点抢救：文本类应用自动
//! 转存最后可恢复快照到临时区，重开可寻回）。
//!
//! 实装：挂起判定器（心跳超时 5s 注入）；蒙层规格（20% 暗化 + 标注）；
/// 两选一浮条（默认不动——超时自动收起）；抢救快照（转存临时区 +
/// 重开寻回链）；体面终止流程（先抢救后结束）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 无响应判定阈值（ms）。
pub const HANG_THRESHOLD_MS: u64 = 5_000;
/// 蒙层暗化（20%——主册定值）。
pub const DIM_PCT: u8 = 20;
/// 浮条自动收起（再等 5s）。
pub const FLOATER_DISMISS_MS: u64 = 5_000;

/// 应用心跳状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HangState {
    Responsive,
    /// 挂起（蒙层+标注+浮条）。
    Suspended,
    Recovered,
    Terminated,
}

/// 应用无响应监视器。
pub struct HangMonitor {
    pub state: HangState,
    /// 最近一次心跳时戳（ms 注入）。
    pub last_heartbeat_ms: u64,
    /// 恢复滞回计数（连续心跳才判活）。
    pub recover_streak: u32,
}

impl HangMonitor {
    pub fn new() -> HangMonitor {
        HangMonitor { state: HangState::Responsive, last_heartbeat_ms: 0, recover_streak: 0 }
    }

    pub fn heartbeat(&mut self, now_ms: u64) {
        self.last_heartbeat_ms = now_ms;
        if self.state == HangState::Suspended {
            // 滞回：恢复需要连续 RECOVER_STREAK 次心跳——「可能只是慢」
            // 的防抖（一次心跳不能证明主线程活了，连续 3 次才算）。
            self.recover_streak += 1;
            if self.recover_streak >= RECOVER_STREAK {
                self.state = HangState::Recovered;
                self.recover_streak = 0;
            }
        }
    }

    /// 周期检查（now_ms 注入）：超 5s 无心跳 → 判定挂起。
    pub fn check(&mut self, now_ms: u64) -> HangState {
        if self.state == HangState::Terminated {
            return self.state;
        }
        if now_ms.saturating_sub(self.last_heartbeat_ms) > HANG_THRESHOLD_MS {
            self.state = HangState::Suspended;
        }
        self.state
    }

    /// 蒙层规格（渲染层直读：暗化 20% + 标注文案，内容冻结最后一帧）。
    pub fn overlay_spec() -> (&'static str, u8) {
        ("无响应——等待或结束", DIM_PCT)
    }

    /// 两选一浮条：默认不动（默认选择=等待；5s 无操作自动收起）。
    pub fn floater_default() -> &'static str {
        "等待"
    }
}

/// 恢复滞回（次心跳——连续 3 次心跳才判恢复）。
pub const RECOVER_STREAK: u32 = 3;

/// 结束成功率账（判据「结束成功率」——尝试 vs 成功的机判账本）。
#[derive(Default)]
pub struct KillLedger {
    pub attempts: u32,
    pub success: u32,
}

impl KillLedger {
    /// 记一次结束尝试；`ok` 由执行层回报（进程句柄消失=成功）。
    pub fn record(&mut self, ok: bool) {
        self.attempts += 1;
        if ok {
            self.success += 1;
        }
    }

    /// 成功率（千分比；零尝试返回 None——不编造 100%）。
    pub fn success_permille(&self) -> Option<u64> {
        if self.attempts == 0 {
            None
        } else {
            Some(self.success as u64 * 1000 / self.attempts as u64)
        }
    }
}

/// 抢救快照：文本类应用最后可恢复内容（转存临时区，重开可寻回）。
pub struct RescueSnapshot {
    pub app: String,
    pub doc: String,
    pub content: String,
    pub at_min: u64,
}

/// 抢救库（临时区语义：容量有限——每应用最近 3 份）。
pub struct RescueBin {
    snaps: Vec<RescueSnapshot>,
}

pub const RESCUE_CAP_PER_APP: usize = 3;

impl RescueBin {
    pub fn new() -> RescueBin {
        RescueBin { snaps: Vec::new() }
    }

    /// 体面终止的抢救步骤：先转存快照，再登记终止。
    /// 每应用只留最近 [`RESCUE_CAP_PER_APP`] 份（超出淘汰最旧）。
    pub fn rescue_then_terminate(
        &mut self,
        monitor: &mut HangMonitor,
        app: &str,
        doc: &str,
        content: &str,
        at_min: u64,
    ) -> bool {
        self.snaps.push(RescueSnapshot {
            app: String::from(app),
            doc: String::from(doc),
            content: String::from(content),
            at_min,
        });
        loop {
            let count = self.snaps.iter().filter(|s| s.app == app).count();
            if count <= RESCUE_CAP_PER_APP {
                break;
            }
            match self.snaps.iter().position(|s| s.app == app) {
                Some(oldest) => {
                    let _ = self.snaps.remove(oldest);
                }
                None => break,
            }
        }
        monitor.state = HangState::Terminated;
        true
    }

    /// 重开寻回：按应用+文档找最近快照。
    pub fn retrieve(&self, app: &str, doc: &str) -> Option<&RescueSnapshot> {
        self.snaps.iter().rev().find(|s| s.app == app && s.doc == doc)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_notresp_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F284");
    // 5s 判定阈值注入：4999ms 正常、5001ms 挂起。
    let mut m = HangMonitor::new();
    m.heartbeat(1_000);
    let s1 = m.check(1_000 + 4_999);
    set.add("F284 under 5s ok", s1 == HangState::Responsive, "4999ms");
    let s2 = m.check(1_000 + 5_001);
    set.add("F284 over 5s hang", s2 == HangState::Suspended, "5001ms inject");
    // 蒙层视觉与标注。
    let (label, dim) = HangMonitor::overlay_spec();
    set.add(
        "F284 overlay spec",
        label.contains("无响应") && dim == DIM_PCT && DIM_PCT == 20,
        "20% dim + label",
    );
    // 两选一默认不动 + 自动收起。
    set.add(
        "F284 default wait",
        HangMonitor::floater_default() == "等待" && FLOATER_DISMISS_MS == 5_000,
        "no action default",
    );
    // 心跳恢复滞回：单次心跳不算活（可能只是慢），连续 3 次才判恢复。
    m.heartbeat(7_000);
    m.heartbeat(7_500);
    set.add(
        "F284 hysteresis holding",
        m.state == HangState::Suspended,
        "one blip isn't alive",
    );
    m.heartbeat(8_000);
    set.add("F284 streak recovers", m.state == HangState::Recovered, "3 in a row");
    // 抢救快照：记事本长文场景——先抢救后终止，重开寻回。
    let mut m2 = HangMonitor::new();
    m2.heartbeat(0);
    let _ = m2.check(10_000);
    let mut bin = RescueBin::new();
    let long_doc = "长文第一段。长文第二段。……";
    let rescued = bin.rescue_then_terminate(&mut m2, "记事本", "报告.md", long_doc, 100);
    set.add(
        "F284 rescue then terminate",
        rescued && m2.state == HangState::Terminated,
        "content saved first",
    );
    let back = bin.retrieve("记事本", "报告.md");
    set.add(
        "F284 reopen recovers",
        back.map(|s| s.content == long_doc).unwrap_or(false),
        "snapshot intact",
    );
    // 结束成功率：已终止状态不再重复判定。
    let s3 = m2.check(99_999);
    set.add("F284 no re-judge", s3 == HangState::Terminated, "terminal state stable");
    // --- 深化二：结束成功率账（零尝试不编造、全成 1000‰）。 ---
    let mut kills = KillLedger::default();
    set.add("F284 kill no-fabrication", kills.success_permille().is_none(), "no attempts no rate");
    kills.record(true);
    kills.record(true);
    kills.record(false);
    set.add(
        "F284 kill rate honest",
        kills.success_permille() == Some(666) && kills.attempts == 3,
        "2/3 = 666‰",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f284_hang_flow() {
        let set = run_notresp_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F284 自检红 {f}/{p}");
    }

    #[test]
    fn rescue_cap_lru() {
        let mut m = HangMonitor::new();
        let mut bin = RescueBin::new();
        for i in 0..5 {
            let _ = bin.rescue_then_terminate(&mut m, "记事本", "同一文档", "内容", i);
            m.state = HangState::Suspended; // 重置以便继续抢救。
        }
        let hits = bin.snaps.iter().filter(|s| s.doc == "同一文档").count();
        assert!(hits <= RESCUE_CAP_PER_APP, "每应用快照上限 3——LRU 淘汰");
    }
}
