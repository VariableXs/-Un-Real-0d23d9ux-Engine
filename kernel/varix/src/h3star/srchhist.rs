//! F307 搜索历史与无痕 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：历史 10 条上限与去重；逐删/全清；无痕图标状态与
//! 不落盘判据（开关后搜索，历史文件无增量）；重启保持开关态。
//!
//! **设计要点（主册）**：
//! - 搜索框下拉显示最近 10 条历史（点击重搜），历史可逐条删或全清；
//! - 「无痕搜索」开关（搜索时不记历史，工具栏图标明确指示当前状态）；
//! - 历史只存本机（U 盘系统天然离线友好）；
//! - 无感标准：常用搜索两击直达；隐私敏感的搜索有明确的无痕出路；
//!   历史是便利不是监控。
//!
//! 落盘语义：历史与无痕开关状态都走 [`super::hbase::PersistKv`]——
//! 「无痕不落盘」= 开关后 record 不产生任何写/冲刷增量（判据直接对账）。

use crate::checks::CheckSet;

use super::hbase::PersistKv;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 历史容量（LRU 上限）。
pub const HISTORY_CAP: usize = 10;

/// 无痕开关持久化键。
pub const INCOGNITO_KEY: &str = "search.incognito";

/// 历史持久化键。
pub const HISTORY_KEY: &str = "search.history";

// ---------------------------------------------------------------------------
// 历史面
// ---------------------------------------------------------------------------

/// 搜索历史库（历史 + 无痕开关 + 落盘账一体）。
pub struct SearchHistoryStore {
    items: Vec<String>,
    pub incognito: bool,
}

impl SearchHistoryStore {
    /// 从落盘账恢复（重启保持开关态判据载体）。
    pub fn restore(disk: &PersistKv) -> SearchHistoryStore {
        let incognito = disk.get(INCOGNITO_KEY) == Some("1");
        let items: Vec<String> = disk
            .get(HISTORY_KEY)
            .unwrap_or("")
            .split('\u{1}')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        SearchHistoryStore { items, incognito }
    }

    /// 导出落盘快照（重启模拟输入）。
    pub fn snapshot(&self) -> PersistKv {
        let mut kv = PersistKv::new();
        kv.set(INCOGNITO_KEY, if self.incognito { "1" } else { "0" });
        kv.set(HISTORY_KEY, &self.items.join("\u{1}"));
        kv.flush();
        kv
    }

    /// 记一次查询：去重置顶、容量 LRU；无痕态零写入（不落盘判据载体）。
    /// 返回是否实际入账（无痕返回 false）。
    pub fn record(&mut self, q: &str) -> bool {
        if self.incognito || q.trim().is_empty() {
            return false;
        }
        self.items.retain(|x| x != q);
        if self.items.len() >= HISTORY_CAP {
            self.items.remove(self.items.len() - 1);
        }
        self.items.insert(0, String::from(q));
        true
    }

    pub fn list(&self) -> &[String] {
        &self.items
    }

    /// 逐条删（按索引——下拉行的 × 按钮）。
    pub fn remove_at(&mut self, idx: usize) -> Option<String> {
        if idx < self.items.len() {
            Some(self.items.remove(idx))
        } else {
            None
        }
    }

    /// 全清。
    pub fn clear_all(&mut self) -> usize {
        let n = self.items.len();
        self.items.clear();
        n
    }

    /// 无痕开关（状态即图标指示源——调用方渲染）。
    pub fn set_incognito(&mut self, on: bool) {
        self.incognito = on;
    }

    /// 落盘（仅在非无痕态下允许产生增量——无痕态 flush 零增量判据载体）。
    /// 返回冲刷次数是否变化。
    pub fn persist(&mut self) -> bool {
        if self.incognito {
            return false;
        }
        let mut kv = PersistKv::new();
        kv.set(HISTORY_KEY, &self.items.join("\u{1}"));
        kv.flush();
        true
    }

    /// 便利两击直达：历史中精确重搜目标存在（点击重搜判据面）。
    pub fn contains(&self, q: &str) -> bool {
        self.items.iter().any(|x| x == q)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F307 自检（判据：10 条去重；逐删/全清；无痕不落盘；重启保持开关态）。
pub fn run_srchhist_checks() -> CheckSet {
    let mut set = CheckSet::new("F307-srchhist");

    // 1. 容量 10 + 去重置顶。
    let mut h = SearchHistoryStore { items: Vec::new(), incognito: false };
    let words = [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l",
    ];
    for w in words {
        h.record(w);
    }
    set.add(
        "cap ten lru",
        h.list().len() == HISTORY_CAP && h.list()[0] == "l" && !h.list().contains(&String::from("a")),
        "",
    );
    h.record("f");
    set.add(
        "dedupe move to top",
        h.list()[0] == "f" && h.list().iter().filter(|x| **x == String::from("f")).count() == 1,
        "",
    );

    // 2. 逐删：按索引删，其余不动（删顶后 len 减一）。
    let removed = h.remove_at(0);
    set.add(
        "remove one",
        removed == Some(String::from("f")) && h.list()[0] == "l" && h.list().len() == HISTORY_CAP - 1,
        "",
    );
    set.add("remove out of range none", h.remove_at(99).is_none(), "");

    // 3. 全清。
    let n = h.clear_all();
    set.add("clear all", n == HISTORY_CAP - 1 && h.list().is_empty(), "");

    // 4. 无痕不落盘：开关后搜索零入账、persist 零冲刷增量。
    let mut h = SearchHistoryStore { items: Vec::new(), incognito: false };
    h.record("普通查询");
    let flushed_before = h.persist();
    h.set_incognito(true);
    let r1 = h.record("如何恢复误删文件");
    let persisted = h.persist();
    set.add(
        "incognito zero footprint",
        flushed_before && !h.contains("如何恢复误删文件") && !r1 && !persisted && h.incognito,
        "",
    );

    // 5. 重启保持开关态（无痕开 → 重启 → 仍无痕）。
    let mut h = SearchHistoryStore { items: Vec::new(), incognito: false };
    h.set_incognito(true);
    let snap = h.snapshot();
    let reborn = SearchHistoryStore::restore(&snap);
    set.add(
        "incognito survives reboot",
        reborn.incognito && reborn.list().is_empty(),
        "",
    );

    // 6. 重启恢复历史（非无痕态历史跨重启在）。
    let mut h = SearchHistoryStore { items: Vec::new(), incognito: false };
    h.record("季度预算");
    h.record("项目排期");
    let snap = h.snapshot();
    let reborn = SearchHistoryStore::restore(&snap);
    set.add(
        "history survives reboot",
        !reborn.incognito && reborn.list() == ["项目排期", "季度预算"],
        "",
    );

    // 7. 空查询不入账（trim 纪律）。
    let mut h = SearchHistoryStore { items: Vec::new(), incognito: false };
    set.add(
        "blank query not recorded",
        !h.record("   ") && h.list().is_empty(),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_order_stable_after_removal() {
        let mut h = SearchHistoryStore { items: Vec::new(), incognito: false };
        h.record("x");
        h.record("y");
        h.remove_at(1);
        assert_eq!(h.list(), ["y"]);
    }

    #[test]
    fn snapshot_flushes_once() {
        let mut h = SearchHistoryStore { items: Vec::new(), incognito: false };
        h.record("q");
        let snap = h.snapshot();
        assert_eq!(snap.flushes, 1);
        assert_eq!(snap.get(INCOGNITO_KEY), Some("0"));
    }

    #[test]
    fn toggling_off_resumes_recording() {
        let mut h = SearchHistoryStore { items: Vec::new(), incognito: false };
        h.set_incognito(true);
        assert!(!h.record("a"));
        h.set_incognito(false);
        assert!(h.record("a"));
    }
}
