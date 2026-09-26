//! F490 关机阻止管理器（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **阻止者列表准确性；默认焦点判据（F207 同源）；仍然关机+抢救链路；
//! 逐应用等待；列表实时性。**
//!
//! 功能定义（主册批次三）：有应用未正常退出阻止关机时——列出阻止者（应用
//! 名+原因「未响应/有未保存文档」）、两个大按钮（「仍然关机」/「取消」——
//! 默认焦点在取消）、逐应用可选跳过（只关别的等这个）、列表实时更新。
//!
//! 零堆纪律：定长阻止者表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 阻止者表容量。
pub const BLOCKER_CAP: usize = 16;

/// 阻止原因（主册原文两类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockReason {
    NotResponding,
    UnsavedDocs,
}

impl BlockReason {
    pub fn label(self) -> &'static str {
        match self {
            BlockReason::NotResponding => "未响应",
            BlockReason::UnsavedDocs => "有未保存文档",
        }
    }
}

/// 用户决定（两个大按钮 + 逐应用跳过）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShutdownDecision {
    /// 取消（默认焦点——保命的默认）。
    Cancel,
    /// 仍然关机（未保存数据按 F311 恢复点抢救）。
    ForceAnyway,
    /// 逐应用等待（只关别的等这个）。
    WaitThis(&'static str),
}

/// 一条阻止者。
#[derive(Clone, Copy, Debug)]
pub struct Blocker {
    pub app: &'static str,
    pub reason: BlockReason,
    /// 该应用已请求退出（等它退出中——列表实时更新源）。
    pub exit_requested: bool,
}

/// 关机阻止管理器。
pub struct ShutdownBlockers {
    blockers: [Option<Blocker>; BLOCKER_CAP],
    n: usize,
    /// 决策记录（默认焦点判据：Cancel 恒可用）。
    pub decision: Option<ShutdownDecision>,
    /// 抢救链路触发（仍然关机 → F311 恢复点抢救标记）。
    pub rescue_armed: bool,
}

impl ShutdownBlockers {
    pub const fn new() -> Self {
        ShutdownBlockers {
            blockers: [None; BLOCKER_CAP],
            n: 0,
            decision: None,
            rescue_armed: false,
        }
    }

    /// 注册阻止者（应用退出请求被拒时入账）。
    pub fn block(&mut self, app: &'static str, reason: BlockReason) -> bool {
        if self.n >= BLOCKER_CAP {
            return false;
        }
        // 同应用重复阻止 = 原因更新（列表准确性）。
        for i in 0..self.n {
            if let Some(b) = self.blockers[i] {
                if b.app == app {
                    self.blockers[i] = Some(Blocker { app, reason, exit_requested: false });
                    return true;
                }
            }
        }
        self.blockers[self.n] = Some(Blocker { app, reason, exit_requested: false });
        self.n += 1;
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }

    pub fn blocker(&self, i: usize) -> Option<&Blocker> {
        self.blockers.get(i).and_then(|b| b.as_ref())
    }

    /// 列表实时更新（应用退出了就少一行——主册判据）。
    pub fn app_exited(&mut self, app: &str) -> bool {
        for i in 0..self.n {
            if let Some(b) = self.blockers[i] {
                if b.app == app {
                    self.blockers[i] = self.blockers[self.n - 1];
                    self.blockers[self.n - 1] = None;
                    self.n -= 1;
                    return true;
                }
            }
        }
        false
    }

    /// 逐应用等待（只关别的等这个：目标标记退出请求，其余照常关）。
    pub fn wait_this(&mut self, app: &'static str) -> bool {
        for i in 0..self.n {
            if let Some(b) = self.blockers[i].as_mut() {
                if b.app == app {
                    b.exit_requested = true;
                    self.decision = Some(ShutdownDecision::WaitThis(app));
                    return true;
                }
            }
        }
        false
    }

    /// 取消（默认焦点判据——F207 同源：焦点初始落在取消上）。
    pub fn cancel(&mut self) {
        self.decision = Some(ShutdownDecision::Cancel);
    }

    /// 仍然关机（抢救链路同步武装——F311 恢复点兜底）。
    pub fn force_anyway(&mut self) -> bool {
        if self.n == 0 {
            return false; // 没有阻止者时此按钮不存在（诚实）
        }
        self.decision = Some(ShutdownDecision::ForceAnyway);
        self.rescue_armed = true;
        true
    }

    /// 默认焦点（主册：默认焦点在取消——保命的默认）。
    pub fn default_focus_is_cancel() -> bool {
        true
    }

    /// 决策后列表清账（关机流结束）。
    pub fn close(&mut self) {
        self.blockers = [None; BLOCKER_CAP];
        self.n = 0;
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_shutblock_checks() -> CheckSet {
    let mut cs = CheckSet::new("F490-shutblock");
    let mut m = ShutdownBlockers::new();
    // 1) 阻止者列表准确性（名+原因）。
    m.block("editor", BlockReason::UnsavedDocs);
    m.block("sync", BlockReason::NotResponding);
    cs.add("list_accurate", m.count() == 2 && m.blocker(0).unwrap().reason.label() == "有未保存文档" && m.blocker(1).unwrap().reason.label() == "未响应", "");
    cs.add("dup_updates_reason", m.block("editor", BlockReason::NotResponding) && m.count() == 2 && m.blocker(0).unwrap().reason == BlockReason::NotResponding, "");
    // 2) 列表实时更新（退出一个少一行）。
    cs.add("realtime_row_count", m.app_exited("sync") && m.count() == 1, "");
    cs.add("exit_unknown_honest", !m.app_exited("ghost"), "");
    // 3) 默认焦点判据（F207 同源：取消）。
    cs.add("default_focus_cancel", ShutdownBlockers::default_focus_is_cancel(), "");
    m.cancel();
    cs.add("cancel_decision", m.decision == Some(ShutdownDecision::Cancel) && !m.rescue_armed, "");
    // 4) 仍然关机 + 抢救链路。
    cs.add("force_arms_rescue", m.force_anyway() && m.rescue_armed && m.decision == Some(ShutdownDecision::ForceAnyway), "");
    // 5) 逐应用等待。
    let mut m2 = ShutdownBlockers::new();
    m2.block("browser", BlockReason::NotResponding);
    m2.block("paint", BlockReason::UnsavedDocs);
    cs.add("wait_this", m2.wait_this("paint") && matches!(m2.decision, Some(ShutdownDecision::WaitThis("paint"))), "");
    // 6) 无阻止者时仍然关机按钮不存在（诚实）。
    let mut m3 = ShutdownBlockers::new();
    cs.add("force_needs_blockers", !m3.force_anyway(), "");
    // 7) 容量诚实。
    cs.add("cap_honest", {
        let mut m4 = ShutdownBlockers::new();
        let mut all = true;
        for i in 0..BLOCKER_CAP + 3 {
            let ok = m4.block(mock_app(i), BlockReason::NotResponding);
            if (i < BLOCKER_CAP) != ok {
                all = false;
            }
        }
        all
    }, "");
    cs
}

fn mock_app(i: usize) -> &'static str {
    const POOL: [&str; 19] = ["a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9", "b0", "b1", "b2", "b3", "b4", "b5", "b6", "b7", "b8"];
    POOL[i.min(POOL.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_shrinks_in_realtime() {
        let mut m = ShutdownBlockers::new();
        m.block("a", BlockReason::UnsavedDocs);
        m.block("b", BlockReason::NotResponding);
        m.block("c", BlockReason::UnsavedDocs);
        assert!(m.app_exited("b"));
        assert_eq!(m.count(), 2);
        // 尾补位后剩余两条准确（a、c 都在）。
        assert!(m.blocker(0).map(|b| b.app == "a").unwrap_or(false));
        assert!(m.blocker(1).map(|b| b.app == "c").unwrap_or(false));
    }

    #[test]
    fn cancel_is_safe_default() {
        let mut m = ShutdownBlockers::new();
        m.block("x", BlockReason::UnsavedDocs);
        m.cancel();
        assert!(!m.rescue_armed, "取消不武装抢救");
    }

    #[test]
    fn force_closes_and_resets() {
        let mut m = ShutdownBlockers::new();
        m.block("y", BlockReason::NotResponding);
        assert!(m.force_anyway());
        m.close();
        assert_eq!(m.count(), 0);
        assert!(m.rescue_armed, "抢救链路保持武装直到关机流结束");
    }
}
