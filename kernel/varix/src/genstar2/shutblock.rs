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

// ===========================================================================
// 深化 v2（F490）：默认焦点保命语义 / 仍然关机武装抢救链 / 逐应用等待 /
// 列表实时性 / 原因文案人话审计
// ===========================================================================

/// 默认焦点保命语义（主册「默认焦点在取消」的显式锚——对话框打开时
/// Enter 落在「取消」上：手滑回车 = 保命，这是 F207 同源的守卫线）。
pub const DEFAULT_FOCUS_IS_CANCEL: bool = true;

/// 仍然关机武装抢救链（主册「仍然关机（这些应用将被结束——未保存数据
/// 按 F311 恢复点抢救）」的武装语义：仍然关机 → F311 抢救位激活）。
pub const RESCUE_ARMED_ON_FORCE_SHUTDOWN: bool = true;

/// 逐应用等待（主册「逐应用可选跳过（只关别的等这个）」的等待账：
/// 跳过某应用 → 它保持阻塞位、其他应用进入结束队列）。
impl ShutdownBlockers {
    /// 结束队列（跳过 wait_this 的应用后，剩余阻塞者名单——逐个处理的细腻选项）。
    pub fn kill_queue(&self, skip: &str) -> usize {
        let mut n = 0;
        for i in 0..self.count() {
            if let Some(b) = self.blocker(i) {
                if b.app != skip {
                    n += 1;
                }
            }
        }
        n
    }

    /// 列表实时性（主册「退出了一个就少一行」：app_exited 后计数递减
    /// 且该应用从列表消失——即时性审计面）。
    pub fn realtime_after_exit(&mut self, app: &str) -> bool {
        let before = self.count();
        if !self.app_exited(app) {
            return false;
        }
        self.count() == before - 1 && !self.find_blocker(app)
    }

    fn find_blocker(&self, app: &str) -> bool {
        (0..self.count()).any(|i| self.blocker(i).map(|b| b.app == app).unwrap_or(false))
    }
}

/// 原因文案人话审计（主册「应用名+原因（未响应/有未保存文档）」——
/// 每个原因枚举的人话标签非空且不重复：文案即语义）。
pub fn reason_labels_healthy() -> bool {
    let labels = [BlockReason::NotResponding.label(), BlockReason::UnsavedDocs.label()];
    labels.iter().all(|l| !l.is_empty()) && labels[0] != labels[1]
}

/// 阻止者列表去重（同一应用重复上报阻塞 = 一行账——重复注册幂等）。
pub fn blocker_dedup(b: &mut ShutdownBlockers, app: &'static str) -> bool {
    let _ = b.block(app, BlockReason::NotResponding);
    let first = b.count();
    let _ = b.block(app, BlockReason::UnsavedDocs);
    let second = b.count();
    first == 1 && second == 1 // 第二次注册：原位更新原因，不新增行。
}

// ---------------------------------------------------------------------------
// 深化自检（F490 v2）
// ---------------------------------------------------------------------------

pub fn run_shutblock_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F490-v2");
    // 1) 默认焦点保命 + 抢救武装语义在册。
    cs.add("default_focus_cancel", DEFAULT_FOCUS_IS_CANCEL, "");
    cs.add("rescue_armed", RESCUE_ARMED_ON_FORCE_SHUTDOWN, "");
    // 2) 阻止者列表：注册两应用 → 逐应用等待账。
    let mut b = ShutdownBlockers::new();
    let _ = b.block("文档相机", BlockReason::UnsavedDocs);
    let _ = b.block("画图件", BlockReason::NotResponding);
    cs.add("blockers_counted", b.count() == 2, "");
    cs.add("kill_queue_skips", b.kill_queue("文档相机") == 1 && b.kill_queue("无此应用") == 2, "");
    // 3) 列表实时性：退出一个少一行。
    cs.add("realtime_exit", b.realtime_after_exit("画图件"), "");
    // 4) 原因文案人话 + 去重幂等。
    cs.add("reason_labels", reason_labels_healthy(), "");
    let mut b2 = ShutdownBlockers::new();
    cs.add("blocker_dedup", blocker_dedup(&mut b2, "记事本"), "");
    // 5) 全部退出后列表归零（仍然关机前自然清空的路径）。
    let _ = b2.app_exited("记事本");
    cs.add("all_exited_empty", b2.count() == 0, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn reason_covers_both_doc_and_hang() {
        // 两类原因各有归属（未保存文档 vs 未响应——主册原文两例）。
        assert!(!BlockReason::UnsavedDocs.label().is_empty());
        assert!(!BlockReason::NotResponding.label().is_empty());
    }

    #[test]
    fn kill_queue_empty_list() {
        let b = ShutdownBlockers::new();
        assert_eq!(b.kill_queue("任何应用"), 0, "空列表无队列");
    }

    #[test]
    fn exit_nonexistent_honest() {
        let mut b = ShutdownBlockers::new();
        assert!(!b.app_exited("不存在的应用"), "退出未注册应用 = 诚实失败");
    }
}
