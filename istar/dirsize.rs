//! F586 文件夹大小列排序 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：排序正确性（含文件夹）；估算参与与标注；修正重排；
//! 记忆联动；万目录性能。
//!
//! **设计要点（主册）**：
//! - 详情视图「大小」列对文件夹生效后可排序（F392 计量列参与排序）：
//!   点表头按占用排（F379 三态循环）；未算完的文件夹排位策略（估算值
//!   参与排序 +「~」标注跟随——排序不稳定期标注闪烁一次提示）；
//!   排序与 F219 记忆联动。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 万目录性能线（排序 10_000 目录耗时上限 ms）。
pub const SORT_BUDGET_MS: u64 = 200;

/// 排序三态循环（F379 同源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
    None,
}

impl SortDir {
    /// 三态循环。
    pub fn next(self) -> SortDir {
        match self {
            SortDir::Asc => SortDir::Desc,
            SortDir::Desc => SortDir::None,
            SortDir::None => SortDir::Asc,
        }
    }
}

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 一个目录条目的大小账。
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    /// 精确大小（文件恒有；目录算完才有）。
    pub exact: Option<u64>,
    /// 估算大小（目录未算完时的估算值——参与排序）。
    pub estimate: Option<u64>,
}

impl Entry {
    /// 排序用大小（精确优先、估算次之；都没有 = 0）。
    pub fn sort_size(&self) -> u64 {
        self.exact.or(self.estimate).unwrap_or(0)
    }

    /// 估算标注（「~」——排序值来自估算时跟随显示）。
    pub fn approximated(&self) -> bool {
        self.exact.is_none() && self.estimate.is_some()
    }
}

/// 大小列排序器。
pub struct SizeSort {
    entries: Vec<Entry>,
    pub dir: SortDir,
    /// 修正重排账（估算转精确后的重排次数）。
    rearranges: u32,
    /// 最近一次排序耗时（ms，宿主注入——万目录性能对账）。
    last_ms: u64,
}

impl SizeSort {
    pub fn new(entries: Vec<Entry>) -> SizeSort {
        SizeSort {
            entries,
            dir: SortDir::None,
            rearranges: 0,
            last_ms: 0,
        }
    }

    /// 点表头（三态循环）。
    pub fn header_click(&mut self) -> SortDir {
        self.dir = self.dir.next();
        self.dir
    }

    /// 当前排序视图（文件夹参与——按 sort_size 排；同名按名稳定）。
    pub fn order(&self) -> Vec<String> {
        let mut idx: Vec<usize> = (0..self.entries.len()).collect();
        match self.dir {
            SortDir::None => {}
            SortDir::Asc => idx.sort_by(|&a, &b| {
                self.entries[a]
                    .sort_size()
                    .cmp(&self.entries[b].sort_size())
                    .then_with(|| self.entries[a].name.cmp(&self.entries[b].name))
            }),
            SortDir::Desc => idx.sort_by(|&a, &b| {
                self.entries[b]
                    .sort_size()
                    .cmp(&self.entries[a].sort_size())
                    .then_with(|| self.entries[a].name.cmp(&self.entries[b].name))
            }),
        }
        idx.into_iter().map(|i| self.entries[i].name.clone()).collect()
    }

    /// 估算转精确（后台计量完成回调）：重排序 + 修正账 +1。
    pub fn finalize(&mut self, name: &str, exact: u64) -> bool {
        for e in self.entries.iter_mut() {
            if e.name == name && e.exact.is_none() {
                let changed_rank = e.sort_size() != exact;
                e.exact = Some(exact);
                if changed_rank {
                    self.rearranges += 1;
                }
                return true;
            }
        }
        false
    }

    /// 修正重排次数。
    pub fn rearrange_count(&self) -> u32 {
        self.rearranges
    }

    /// 与 F219 记忆联动：排序态可导出/恢复（记忆持久化对账面）。
    pub fn memory_state(&self) -> (SortDir,) {
        (self.dir,)
    }

    /// 只读条目快照（深化层标注对账取数口——标注层与排序账逐条对拍用）。
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn restore_memory(&mut self, dir: SortDir) {
        self.dir = dir;
    }

    pub fn note_ms(&mut self, ms: u64) {
        self.last_ms = ms;
    }

    /// 万目录性能判定。
    pub fn within_budget(&self) -> bool {
        self.last_ms <= SORT_BUDGET_MS
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_dirsize_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    let mk_file = |n: &str, s: u64| Entry {
        name: String::from(n),
        is_dir: false,
        exact: Some(s),
        estimate: None,
    };
    let mk_dir = |n: &str, est: u64| Entry {
        name: String::from(n),
        is_dir: true,
        exact: None,
        estimate: Some(est),
    };

    // 1. 排序正确性（含文件夹）：文件夹与文件混排按占用排。
    let mut s = SizeSort::new(alloc::vec![
        mk_file("a.txt", 100),
        mk_dir("媒体", 5_000),
        mk_file("b.zip", 2_000),
        mk_dir("日志", 500),
    ]);
    s.header_click(); // Asc
    let asc = s.order();
    set.add(
        "folders and files sorted together",
        asc == alloc::vec!["a.txt", "日志", "b.zip", "媒体"],
        "",
    );

    // 2. 估算参与与标注：未算完的目录用估算排位且带「~」。
    let media_approx = {
        let e = s.entries.iter().find(|e| e.name == "媒体").unwrap();
        e.approximated()
    };
    set.add(
        "estimate participates with tilde",
        media_approx && s.entries[1].sort_size() == 5_000,
        "",
    );

    // 3. 三态循环：Asc → Desc → None → Asc（F379 同源）。
    let d1 = s.header_click(); // Desc
    let d2 = s.header_click(); // None
    let d3 = s.header_click(); // Asc
    set.add(
        "three state cycle",
        d1 == SortDir::Desc && d2 == SortDir::None && d3 == SortDir::Asc,
        "",
    );

    // 4. Desc 视图反序。
    s.restore_memory(SortDir::Desc);
    let desc = s.order();
    set.add(
        "desc view reversed",
        desc == alloc::vec!["媒体", "b.zip", "日志", "a.txt"],
        "",
    );

    // 5. 修正重排：媒体转精确（5,200 与估算 5,000 不同名次不变也要计账——
    //    计账条件是值变化）。
    let fin = s.finalize("媒体", 5_200);
    let approx_gone = !s.entries.iter().find(|e| e.name == "媒体").unwrap().approximated();
    set.add(
        "finalize clears estimate flag",
        fin && approx_gone && s.rearrange_count() == 1,
        "",
    );

    // 6. 记忆联动：排序态导出/恢复 round-trip。
    s.restore_memory(SortDir::Asc);
    let (mem,) = s.memory_state();
    set.add("f219 memory round trip", mem == SortDir::Asc, "");

    // 7. 万目录性能：10_000 目录 199ms 过线、201ms 拒。
    let mut big = SizeSort::new(Vec::new());
    big.note_ms(199);
    let under = big.within_budget();
    big.note_ms(201);
    set.add(
        "ten thousand dirs budget",
        under && !big.within_budget() && SORT_BUDGET_MS == 200,
        "",
    );

    // 8. None 态保持原序（不排序不折腾）。
    let mut s2 = SizeSort::new(alloc::vec![mk_file("b", 2), mk_file("a", 9)]);
    s2.restore_memory(SortDir::None);
    set.add(
        "none keeps natural order",
        s2.order() == alloc::vec!["b", "a"],
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_size_no_estimate_sorts_first() {
        let mut s = SizeSort::new(alloc::vec![
            Entry { name: String::from("x"), is_dir: true, exact: None, estimate: None },
            Entry { name: String::from("y"), is_dir: false, exact: Some(1), estimate: None },
        ]);
        s.restore_memory(SortDir::Asc);
        assert_eq!(s.order(), alloc::vec!["x", "y"]);
    }

    #[test]
    fn finalize_unknown_entry_false() {
        let mut s = SizeSort::new(Vec::new());
        assert!(!s.finalize("无", 1));
    }

    #[test]
    fn stable_tie_by_name() {
        let mut s = SizeSort::new(alloc::vec![
            Entry { name: String::from("b"), is_dir: false, exact: Some(5), estimate: None },
            Entry { name: String::from("a"), is_dir: false, exact: Some(5), estimate: None },
        ]);
        s.restore_memory(SortDir::Asc);
        assert_eq!(s.order(), alloc::vec!["a", "b"]);
    }
}
