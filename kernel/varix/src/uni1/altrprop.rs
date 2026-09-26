//! F412 Alt+Enter 属性快捷 · 完整设计（STAR I 主册 G-I-12）。
//!
//! **判据（主册）**：单/多选两形制；合计计算准确性（F392 计量服务同源）；
//! 对话框记忆联动；键位注册；从列表关闭焦点回归。＋通12。
//!
//! 设计：属性对话框语义核——单选形制（单项全字段）与多选形制（合计页：
//! N 项/总大小/类型分布）；合计走注入式计量（调用方把 F392 同源读数
//! 送进来，模块只做准确性核算）；对话框位置记忆联动（F382 键注入）；
//! 关闭后焦点回归清单（回归到触发选择集——记账）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 多选合计（类型分布以 (类型键, 计数) 表示）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AggSummary {
    pub count: u64,
    pub total_bytes: u64,
    /// 类型分布（类型键 → 项数）。
    pub type_dist: Vec<(u64, u64)>,
}

/// 属性对话框状态机。
pub struct PropDialog {
    /// 单选模式目标（None = 多选模式）。
    pub single: Option<u64>,
    /// 多选集合。
    pub multi: Vec<u64>,
    /// 最近一次合计（对账基准）。
    pub last_agg: Option<AggSummary>,
    /// 位置记忆（F382 键——重启后回到记忆位）。
    pub pos_mem_key: Option<u64>,
    pub pos: (u32, u32),
    /// 打开/关闭账。
    pub open_count: u64,
    /// 焦点回归成败账（每次关闭必须回归，违例计数）。
    pub focus_returned: u64,
    pub focus_lost: u64,
    pub is_open: bool,
}

impl PropDialog {
    pub fn new() -> PropDialog {
        PropDialog {
            single: None,
            multi: Vec::new(),
            last_agg: None,
            pos_mem_key: None,
            pos: (0, 0),
            open_count: 0,
            focus_returned: 0,
            focus_lost: 0,
            is_open: false,
        }
    }

    /// Alt+Enter：单选打开。
    pub fn open_single(&mut self, id: u64, pos: (u32, u32)) {
        self.single = Some(id);
        self.multi.clear();
        self.is_open = true;
        self.open_count += 1;
        self.apply_pos_memory(pos);
    }

    /// Alt+Enter：多选打开（≥2 项 → 合计形制）。
    pub fn open_multi(&mut self, ids: Vec<u64>, pos: (u32, u32)) {
        self.single = None;
        self.multi = ids;
        self.is_open = true;
        self.open_count += 1;
        self.apply_pos_memory(pos);
    }

    fn apply_pos_memory(&mut self, default_pos: (u32, u32)) {
        self.pos = match self.pos_mem_key {
            Some(_) => self.pos, // 有记忆 → 回记忆位
            None => default_pos, // 无记忆 → 默认位（列表旁）
        };
    }

    /// 合计计算（F392 同源读数注入：id → (bytes, type_key)）。
    /// 合计准确性判据的核算点：count/total/类型分布逐项核对。
    pub fn compute_agg(&mut self, meter: &[(u64, u64, u64)]) -> Option<AggSummary> {
        if self.single.is_some() || self.multi.is_empty() {
            return None; // 合计只属多选形制。
        }
        let mut agg = AggSummary { count: 0, total_bytes: 0, type_dist: Vec::new() };
        for id in &self.multi {
            if let Some((_, bytes, ty)) = meter.iter().find(|(i, _, _)| i == id) {
                agg.count += 1;
                agg.total_bytes += bytes;
                match agg.type_dist.iter_mut().find(|(t, _)| t == ty) {
                    Some((_, c)) => *c += 1,
                    None => agg.type_dist.push((*ty, 1)),
                }
            }
        }
        agg.type_dist.sort_unstable();
        self.last_agg = Some(agg.clone());
        Some(agg)
    }

    /// 关闭：焦点回归到触发选择集（回归账 +1；调用方失败路径走 report）。
    pub fn close(&mut self, focus_returned: bool) -> bool {
        if !self.is_open {
            return false;
        }
        self.is_open = false;
        if focus_returned {
            self.focus_returned += 1;
        } else {
            self.focus_lost += 1;
        }
        true
    }

    /// 多选形制判定（供界面层形制选择）。
    pub fn is_agg_mode(&self) -> bool {
        self.single.is_none() && self.multi.len() >= 2
    }
}

pub fn run_altrprop_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F412");
    let mut d = PropDialog::new();
    // 单选形制。
    d.open_single(7, (100, 80));
    set.add(
        "f412-single-mode",
        d.single == Some(7) && d.is_open && !d.is_agg_mode() && d.pos == (100, 80),
        "",
    );
    set.add("f412-close-focus-return", d.close(true) && d.focus_returned == 1, "");

    // 多选形制 + 合计准确性（F392 同源注入）。
    d.pos_mem_key = Some(1);
    d.pos = (300, 200);
    d.open_multi(alloc::vec![1, 2, 3, 4], (150, 150));
    set.add(
        "f412-multi-mode",
        d.is_agg_mode() && d.pos == (300, 200),
        "",
    );
    let agg = d.compute_agg(&[(1, 100, 1), (2, 250, 1), (3, 50, 2), (4, 600, 3)]);
    set.add(
        "f412-agg-accurate",
        agg.as_ref().map(|a| {
            a.count == 4
                && a.total_bytes == 1_000
                && a.type_dist == alloc::vec![(1, 2), (2, 1), (3, 1)]
        }).unwrap_or(false),
        "",
    );
    // 集合外 id 不虚计（count < multi.len() 即暴露）。
    d.open_multi(alloc::vec![1, 99], (0, 0));
    let agg2 = d.compute_agg(&[(1, 10, 1)]);
    set.add(
        "f412-agg-missing-id-honest",
        agg2.as_ref().map(|a| a.count == 1 && a.total_bytes == 10).unwrap_or(false),
        "",
    );
    // 单选形制拒绝合计（形制纪律）。
    d.open_single(1, (0, 0));
    set.add("f412-single-no-agg", d.compute_agg(&[(1, 10, 1)]).is_none(), "");
    // 关闭焦点回归账与违例账分开记。
    set.add(
        "f412-focus-ledger",
        d.close(false) && d.focus_lost == 1 && d.focus_returned == 1,
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pos_memory_roundtrip() {
        let mut d = PropDialog::new();
        d.open_single(1, (50, 60));
        assert_eq!(d.pos, (50, 60), "无记忆用默认位");
        d.pos = (400, 300);
        d.pos_mem_key = Some(9);
        d.open_single(2, (10, 10));
        assert_eq!(d.pos, (400, 300), "有记忆回记忆位");
    }

    #[test]
    fn agg_type_distribution_sorted() {
        let mut d = PropDialog::new();
        d.open_multi(alloc::vec![5, 6, 7, 8], (0, 0));
        let a = d.compute_agg(&[(5, 1, 3), (6, 1, 1), (7, 1, 3), (8, 1, 1)]).unwrap();
        assert_eq!(a.type_dist, alloc::vec![(1, 2), (3, 2)]);
        assert_eq!(a.total_bytes, 4);
    }
}
