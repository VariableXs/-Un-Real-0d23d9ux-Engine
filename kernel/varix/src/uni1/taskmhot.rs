//! F402 任务管理器快捷入口 · 完整设计（STAR I 主册 G-I-02）。
//!
//! **判据（主册）**：三入口快捷键注册（F244）；刷新 1s±0.1s；排序/结束
//! 任务用例；三处数据对账（同进程同读数误差 <3%）。＋通12。
//!
//! 设计：Ctrl+Shift+Esc 直达 + Win+X F402 项 + Ctrl+Alt+Del 安全屏卡
//! 三入口同注册表；进程读数采样账（1s 刷新抖动记账）；列头排序（数值
//! 序/名称序）；结束任务确认链；三源对账（本界面/F354 托盘/F291 耗电
//! 排行同读数误差容差 3%——读数由调用方注入对拍）。

use crate::checks::CheckSet;
use crate::uni1::ubase::{Chord, HotkeyTable, MOD_CTRL, MOD_SHIFT};

use alloc::vec::Vec;

/// 刷新周期判线（ms）——「1s±0.1s」。
pub const REFRESH_PERIOD_MS: u64 = 1_000;
pub const REFRESH_TOLERANCE_MS: u64 = 100;
/// 三处对账容差（%）。
pub const RECONCILE_TOLERANCE_PCT: u64 = 3;

/// 一行进程读数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcRow {
    pub pid: u64,
    pub name: &'static str,
    pub cpu_permille: u64,
    pub mem_kb: u64,
    /// 无响应标记（F284 判定衔接——结束任务免确认直达）。
    pub not_responding: bool,
}

pub enum SortKey {
    Cpu,
    Mem,
    Name,
}

/// 任务管理器语义核。
pub struct TaskMgr {
    pub hotkeys: HotkeyTable,
    rows: Vec<ProcRow>,
    pub sort: SortKey,
    /// 最近 N 次刷新间隔（抖动账）。
    pub refresh_gaps_ms: Vec<u64>,
    /// 结束任务计数（确认链/免确认分开记）。
    pub end_confirmed: u64,
    pub end_direct: u64,
}

impl TaskMgr {
    pub fn new() -> TaskMgr {
        let mut hotkeys = HotkeyTable::new();
        // 三入口：Ctrl+Shift+Esc 直达 / Win+X 菜单项（f408 分发）/
        // Ctrl+Alt+Del 安全屏卡（F406 分发）——后两者经动作 ID 路由，
        // 注册表登记直达键。
        let _ = hotkeys.register("f402.taskmgr", Chord::new(MOD_CTRL | MOD_SHIFT, 0x1B));
        TaskMgr {
            hotkeys,
            rows: Vec::new(),
            sort: SortKey::Cpu,
            refresh_gaps_ms: Vec::new(),
            end_confirmed: 0,
            end_direct: 0,
        }
    }

    /// 采样注入（1s 周期由调用方驱动；间隔超差如实记账）。
    pub fn sample(&mut self, rows: Vec<ProcRow>, gap_ms: u64) {
        self.rows = rows;
        self.refresh_gaps_ms.push(gap_ms);
        if self.refresh_gaps_ms.len() > 64 {
            self.refresh_gaps_ms.remove(0);
        }
    }

    /// 刷新抖动是否全部在判线内（1s±0.1s）。
    pub fn refresh_in_budget(&self) -> bool {
        self.refresh_gaps_ms.iter().all(|g| {
            *g + REFRESH_TOLERANCE_MS >= REFRESH_PERIOD_MS
                && *g <= REFRESH_PERIOD_MS + REFRESH_TOLERANCE_MS
        })
    }

    /// 列头排序：数值降序、名称升序；平局按 pid 升序（确定可复现）。
    pub fn sorted_view(&self) -> Vec<u64> {
        let mut idx: Vec<usize> = (0..self.rows.len()).collect();
        // less(a, b) = a 应排在 b 前。数值列降序（值大者前），平局 pid
        // 升序；名称列升序，平局 pid 升序。
        let less = |a: &ProcRow, b: &ProcRow| match self.sort {
            SortKey::Cpu => {
                a.cpu_permille > b.cpu_permille
                    || (a.cpu_permille == b.cpu_permille && a.pid < b.pid)
            }
            SortKey::Mem => a.mem_kb > b.mem_kb || (a.mem_kb == b.mem_kb && a.pid < b.pid),
            SortKey::Name => {
                a.name.cmp(b.name) == core::cmp::Ordering::Less
                    || (a.name == b.name && a.pid < b.pid)
            }
        };
        // 插入排序（n 小；不引 sort 依赖）。
        for i in 1..idx.len() {
            let mut j = i;
            while j > 0 {
                let swap = less(&self.rows[idx[j]], &self.rows[idx[j - 1]]);
                if swap {
                    idx.swap(j - 1, j);
                    j -= 1;
                } else {
                    break;
                }
            }
        }
        idx.iter().map(|i| self.rows[*i].pid).collect()
    }

    /// 结束任务：无响应进程免确认直达；健康进程走确认链。
    pub fn end_task(&mut self, pid: u64, user_confirmed: bool) -> bool {
        match self.rows.iter().find(|r| r.pid == pid) {
            Some(r) if r.not_responding => {
                self.end_direct += 1;
                self.rows.retain(|r| r.pid != pid);
                true
            }
            Some(_) if user_confirmed => {
                self.end_confirmed += 1;
                self.rows.retain(|r| r.pid != pid);
                true
            }
            _ => false,
        }
    }

    /// 三源对账：本界面读数 vs 注入的另两源同进程读数（误差 <3%）。
    pub fn reconcile(&self, pid: u64, tray_val: u64, power_val: u64) -> bool {
        let Some(mine) = self.rows.iter().find(|r| r.pid == pid) else {
            return false;
        };
        let base = mine.cpu_permille.max(1);
        let ok = |v: u64| {
            let d = if v > base { v - base } else { base - v };
            d * 100 <= base * RECONCILE_TOLERANCE_PCT
        };
        ok(tray_val) && ok(power_val)
    }

    pub fn rows(&self) -> &[ProcRow] {
        &self.rows
    }
}

pub fn run_taskmhot_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F402");
    let mut t = TaskMgr::new();
    set.add(
        "f402-hotkey-registered",
        t.hotkeys.lookup(Chord::new(MOD_CTRL | MOD_SHIFT, 0x1B)) == Some("f402.taskmgr"),
        "",
    );
    t.sample(
        alloc::vec![
            ProcRow { pid: 1, name: "shell", cpu_permille: 120, mem_kb: 4096, not_responding: false },
            ProcRow { pid: 2, name: "editor", cpu_permille: 800, mem_kb: 65536, not_responding: false },
            ProcRow { pid: 3, name: "hung", cpu_permille: 0, mem_kb: 1024, not_responding: true },
        ],
        REFRESH_PERIOD_MS,
    );
    set.add("f402-refresh-budget", t.refresh_in_budget(), "");
    t.sort = SortKey::Cpu;
    set.add("f402-sort-cpu", t.sorted_view() == alloc::vec![2, 1, 3], "");
    t.sort = SortKey::Name;
    set.add("f402-sort-name", t.sorted_view() == alloc::vec![2, 3, 1], ""); // editor<hung<shell
    // 结束任务：无响应免确认；健康需确认。
    set.add(
        "f402-end-hung-direct",
        t.end_task(3, false) && t.end_direct == 1,
        "",
    );
    set.add("f402-end-healthy-needs-confirm", !t.end_task(1, false) && t.end_task(1, true), "");
    // 三源对账：2% 误差过、5% 误差拒。
    set.add("f402-reconcile-within-3pct", t.reconcile(2, 816, 784), "");
    set.add("f402-reconcile-beyond-3pct", !t.reconcile(2, 840, 800), "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pid: u64, cpu: u64, mem: u64, name: &'static str) -> ProcRow {
        ProcRow { pid, name, cpu_permille: cpu, mem_kb: mem, not_responding: false }
    }

    #[test]
    fn sort_orders_deterministic() {
        let mut t = TaskMgr::new();
        t.sample(
            alloc::vec![row(1, 500, 100, "b"), row(2, 500, 900, "b"), row(3, 100, 500, "a")],
            1_000,
        );
        t.sort = SortKey::Mem;
        assert_eq!(t.sorted_view(), alloc::vec![2, 3, 1]);
        t.sort = SortKey::Name;
        assert_eq!(t.sorted_view(), alloc::vec![3, 1, 2], "a<b；同名平局按 pid 升序");
        t.sort = SortKey::Cpu;
        assert_eq!(t.sorted_view(), alloc::vec![1, 2, 3], "CPU 平局 500/500 按 pid 升序");
    }

    #[test]
    fn refresh_gap_drift_honest() {
        let mut t = TaskMgr::new();
        t.sample(alloc::vec![], 1_050);
        t.sample(alloc::vec![], 950);
        assert!(t.refresh_in_budget());
        t.sample(alloc::vec![], 1_200);
        assert!(!t.refresh_in_budget(), "1.2s 超差必须可见");
    }

    #[test]
    fn reconcile_tolerance_math() {
        let mut t = TaskMgr::new();
        t.sample(alloc::vec![row(9, 1_000, 0, "x")], 1_000);
        assert!(t.reconcile(9, 1_029, 971), "±2.9% 过");
        assert!(!t.reconcile(9, 1_031, 1_000), "±3.1% 拒");
    }
}
