//! F175 崩溃隔离 · 批次七深化（v7）——隔离审查队列、升级解除调度、
//! 按模块崩溃相关性、隔离期资源冻结账。零堆、no_std。

use crate::checks::CheckSet;

/// 审查队列容量。
pub const REVIEW_CAP: usize = 12;
/// 自动解除隔离的静默夜数（升级后 7 夜无复发 → 可解除——给悔改机会）。
pub const UNQUARANTINE_QUIET_NIGHTS: u32 = 7;
/// 相关性窗口：同模块 N 天内 ≥3 崩 = 系统性（与主层风暴线对齐）。
pub const CORR_WINDOW_DAYS: u32 = 7;
pub const CORR_THRESHOLD: u32 = 3;
/// 冻结资源类别数。
pub const FREEZE_KINDS: usize = 4;

/// 隔离审查条目：被隔离应用 + 隔离日起。
#[derive(Clone, Copy, PartialEq)]
pub struct ReviewEntry {
    pub app_id: u32,
    pub quarantined_day: u32,
    /// 最后一次复发日（无复发 = 0 哨兵不好——用 Option 语义的 0xFFFFFFFF）。
    pub last_relapse_day: u32,
}

pub const NO_RELAPSE: u32 = 0xFFFF_FFFF;

/// 审查队列：FIFO，解除条件 = 静默 ≥ UNQUARANTINE_QUIET_NIGHTS。
#[derive(Clone, Copy)]
pub struct ReviewQueue {
    entries: [Option<ReviewEntry>; REVIEW_CAP],
    pub n: usize,
}

impl ReviewQueue {
    pub const fn new() -> ReviewQueue {
        ReviewQueue { entries: [None; REVIEW_CAP], n: 0 }
    }

    pub fn admit(&mut self, e: ReviewEntry) -> bool {
        if self.n >= REVIEW_CAP {
            return false;
        }
        for i in 0..self.n {
            if self.entries[i].unwrap().app_id == e.app_id {
                return false; // 已在审查中不重复收
            }
        }
        self.entries[self.n] = Some(e);
        self.n += 1;
        true
    }

    /// 记一次复发（刷新 last_relapse_day——静默计时重置的依据）。
    pub fn relapse(&mut self, app_id: u32, day: u32) -> bool {
        for e in self.entries.iter_mut().take(self.n) {
            if let Some(x) = e {
                if x.app_id == app_id {
                    x.last_relapse_day = day;
                    return true;
                }
            }
        }
        false
    }

    /// 解除判定：静默夜数 = today - max(quarantine_day, last_relapse_day)。
    /// 无复发用 quarantine_day 起算。
    pub fn quiet_nights(&self, app_id: u32, today: u32) -> Option<u32> {
        for e in self.entries.iter().take(self.n) {
            let x = e.unwrap();
            if x.app_id == app_id {
                // 无复发哨兵不参与锚点（0xFFFFFFFF 当天数会算出 0——语义错误）。
                let anchor = if x.last_relapse_day == NO_RELAPSE {
                    x.quarantined_day
                } else {
                    x.last_relapse_day.max(x.quarantined_day)
                };
                return Some(today.saturating_sub(anchor));
            }
        }
        None
    }

    /// 解除出队（真移除——不是打标记让死数据烂在环里）。
    pub fn release(&mut self, app_id: u32) -> bool {
        for i in 0..self.n {
            if self.entries[i].unwrap().app_id == app_id {
                for j in i..self.n - 1 {
                    self.entries[j] = self.entries[j + 1];
                }
                self.entries[self.n - 1] = None;
                self.n -= 1;
                return true;
            }
        }
        false
    }

    /// 待解除名单：静默够夜数的 app 列表（返回数量并填 out）。
    pub fn due_for_release(&self, today: u32, out: &mut [u32]) -> usize {
        let mut k = 0;
        for e in self.entries.iter().take(self.n) {
            let x = e.unwrap();
            if let Some(q) = self.quiet_nights(x.app_id, today) {
                if q >= UNQUARANTINE_QUIET_NIGHTS && k < out.len() {
                    out[k] = x.app_id;
                    k += 1;
                }
            }
        }
        k
    }
}

/// 按模块崩溃相关性：环记录 (模块, 天)，窗口内计数定级。
#[derive(Clone, Copy)]
pub struct ModuleCrashLedger {
    modules: [u8; 32],
    days: [u32; 32],
    pub n: usize,
}

impl ModuleCrashLedger {
    pub const fn new() -> ModuleCrashLedger {
        ModuleCrashLedger { modules: [0; 32], days: [0; 32], n: 0 }
    }

    pub fn record(&mut self, module: u8, day: u32) {
        if self.n < 32 {
            self.modules[self.n] = module;
            self.days[self.n] = day;
            self.n += 1;
        } else {
            // 环满：整体前移（丢最旧）。
            for i in 1..32 {
                self.modules[i - 1] = self.modules[i];
                self.days[i - 1] = self.days[i];
            }
            self.modules[31] = module;
            self.days[31] = day;
        }
    }

    /// 模块在 [day - CORR_WINDOW_DAYS + 1, day] 窗口内的崩溃数。
    pub fn crashes_in_window(&self, module: u8, day: u32) -> u32 {
        let mut c = 0;
        for i in 0..self.n {
            if self.modules[i] == module {
                let d = self.days[i];
                if d <= day && day - d < CORR_WINDOW_DAYS {
                    c += 1;
                }
            }
        }
        c
    }

    /// 系统性判定：窗口内 ≥ CORR_THRESHOLD → 系统性（不是应用问题）。
    pub fn is_systemic(&self, module: u8, day: u32) -> bool {
        self.crashes_in_window(module, day) >= CORR_THRESHOLD
    }
}

/// 隔离期资源冻结账：四类资源（0=网络 1=文件写 2=设备 3=定时器），
/// 隔离即全冻；解除即按类解冻（冻结是态不是一次性动作）。
#[derive(Clone, Copy)]
pub struct FreezeLedger {
    frozen: [bool; FREEZE_KINDS],
    pub transitions: u32,
}

impl FreezeLedger {
    pub const fn new() -> FreezeLedger {
        FreezeLedger { frozen: [false; FREEZE_KINDS], transitions: 0 }
    }

    /// 隔离生效：全类置冻（幂等——重复隔离不重复计账）。
    pub fn freeze_all(&mut self) {
        for i in 0..FREEZE_KINDS {
            if !self.frozen[i] {
                self.frozen[i] = true;
                self.transitions += 1;
            }
        }
    }

    /// 解除：全类解冻（同样幂等——解除是冻结的对称闭环）。
    pub fn defrost_all(&mut self) {
        for i in 0..FREEZE_KINDS {
            if self.frozen[i] {
                self.frozen[i] = false;
                self.transitions += 1;
            }
        }
    }

    pub fn is_frozen(&self, kind: usize) -> Option<bool> {
        self.frozen.get(kind).copied()
    }

    pub fn all_frozen(&self) -> bool {
        self.frozen.iter().all(|f| *f)
    }

    pub fn all_thawed(&self) -> bool {
        self.frozen.iter().all(|f| !*f)
    }
}

#[inline(never)]
pub fn run_crashiso_b7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F175-b7");

    // 1) 审查准入：入队成功、重复应用拒、满容拒。
    let mut q = ReviewQueue::new();
    let ok = q.admit(ReviewEntry { app_id: 1, quarantined_day: 100, last_relapse_day: NO_RELAPSE });
    let dup = q.admit(ReviewEntry { app_id: 1, quarantined_day: 101, last_relapse_day: NO_RELAPSE });
    cs.add("review_admit_guards", ok && !dup && q.n == 1, "");

    // 2) 静默夜数：隔离日 100、今天 105 → 5 夜（无复发以隔离日起算）。
    cs.add("review_quiet_from_quarantine", q.quiet_nights(1, 105) == Some(5), "");

    // 3) 复发重置静默钟：第 103 天复发 → 今天 105 静默 2 夜（不是 5）。
    assert!(q.relapse(1, 103));
    cs.add("review_relapse_resets_clock", q.quiet_nights(1, 105) == Some(2), "");

    // 4) 解除线：静默 7 夜够、6 夜不够（线值恰点）。
    cs.add(
        "review_release_line",
        q.quiet_nights(1, 110) == Some(7) && q.quiet_nights(1, 109) == Some(6),
        "",
    );

    // 5) 待解除名单：两个应用分别达标/未达标 → 名单只含达标者。
    q.admit(ReviewEntry { app_id: 2, quarantined_day: 104, last_relapse_day: NO_RELAPSE });
    let mut due = [0u32; REVIEW_CAP];
    let k = q.due_for_release(110, &mut due); // app1 静默 7 夜达标、app2 静默 6 夜未达
    cs.add("review_due_list", k == 1 && due[0] == 1, "");

    // 6) 解除出队：release 后 n 减、再 release 同应用 false（真移除非标记）。
    let r1 = q.release(1);
    let r2 = q.release(1);
    cs.add("review_release_removes", r1 && !r2 && q.n == 1, "");

    // 7) 相关性：同模块 7 天窗内 3 崩 = 系统性；散布窗外不算。
    let mut led = ModuleCrashLedger::new();
    led.record(5, 10);
    led.record(5, 12);
    led.record(5, 16);
    cs.add(
        "corr_systemic_in_window",
        led.crashes_in_window(5, 16) == 3 && led.is_systemic(5, 16),
        "",
    );

    // 8) 窗口边界：day 17 时第 10 天的崩出窗（10 < 17-7+1=11）→ 计数降 2。
    cs.add(
        "corr_window_boundary",
        led.crashes_in_window(5, 17) == 2 && !led.is_systemic(5, 17),
        "",
    );

    // 9) 模块隔离：模块 6 的崩不计入模块 5（归因不串）。
    led.record(6, 15);
    cs.add("corr_module_isolated", led.crashes_in_window(5, 16) == 3 && led.crashes_in_window(6, 16) == 1, "");

    // 10) 冻结账：隔离 → 全冻（4 次迁变）；解除 → 全解（再 4 次）；幂等不重复计。
    let mut f = FreezeLedger::new();
    f.freeze_all();
    let t1 = f.transitions;
    f.freeze_all(); // 幂等
    let t2 = f.transitions;
    cs.add(
        "freeze_idempotent",
        f.all_frozen() && t1 == FREEZE_KINDS as u32 && t2 == t1 && f.is_frozen(0) == Some(true),
        "",
    );

    // 11) 冻结-解冻对称闭环：冻 4 迁变 → 解 4 迁变 → 全 thawed；越界 kind → None。
    let mut f2 = FreezeLedger::new();
    f2.freeze_all();
    f2.defrost_all();
    cs.add(
        "freeze_thaw_symmetric",
        f2.all_thawed() && f2.transitions == (FREEZE_KINDS * 2) as u32 && f2.is_frozen(FREEZE_KINDS).is_none(),
        "",
    );

    // 12) 常量自洽：静默 7 夜、窗口 7 天、阈值 3（三个 7/3 支点）。
    cs.add(
        "b7_constants",
        UNQUARANTINE_QUIET_NIGHTS == 7 && CORR_WINDOW_DAYS == 7 && CORR_THRESHOLD == 3,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_full_capacity_honest() {
        // 12 容量守门：13 个不同应用 → 第 13 个诚实拒。
        let mut q = ReviewQueue::new();
        for i in 0..REVIEW_CAP as u32 {
            assert!(q.admit(ReviewEntry { app_id: i, quarantined_day: 1, last_relapse_day: NO_RELAPSE }));
        }
        assert!(!q.admit(ReviewEntry { app_id: 999, quarantined_day: 1, last_relapse_day: NO_RELAPSE }));
        assert_eq!(q.n, REVIEW_CAP);
    }

    #[test]
    fn corr_uses_newest_when_ring_wraps() {
        // 环回卷后旧崩出账：34 次记录只留最近 32——窗口计数随之失忆（诚实口径）。
        let mut led = ModuleCrashLedger::new();
        for d in 0..34u32 {
            led.record(1, d);
        }
        // 最近 32 条是天 2..34；查 day 33 窗 [27,33] → 7 条。
        assert_eq!(led.crashes_in_window(1, 33), 7);
    }

    #[test]
    fn quiet_nights_unknown_app() {
        // 未收审的应用查询 → None（不编造静默夜数）。
        let q = ReviewQueue::new();
        assert!(q.quiet_nights(42, 100).is_none());
    }
}
