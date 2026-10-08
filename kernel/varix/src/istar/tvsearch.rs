//! F563 任务视图搜索 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：过滤即时性 <100ms；跨桌标注与跳转；Enter 聚焦；
//! 空态出路；与 F081/F235 状态同步。
//!
//! **设计要点（主册）**：
//! - 任务视图（F081）内搜索：视图顶部搜索框——输入即过滤窗口
//!   （标题/应用名匹配），结果实时高亮、Enter 跳转聚焦；
//! - 跨桌面搜索（虚拟桌面 F235 的窗口全搜，跨桌结果标注所在桌面可一键跳）；
//! - 无结果时提示「没有匹配窗口——要不要开个新的？」（F210 空态出路）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 过滤即时性红线（ms）。
pub const FILTER_BUDGET_MS: u64 = 100;

/// 空态出路文案（F210 三件套之一）。
pub const EMPTY_GUIDANCE: &str = "没有匹配窗口——要不要开个新的？";

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 任务视图里的一个窗口。
#[derive(Clone, Debug)]
pub struct ViewWin {
    pub id: u64,
    pub title: &'static str,
    pub app: &'static str,
    /// 所在虚拟桌面（F235 桌面条序号，0 基）。
    pub desk: usize,
}

/// 任务视图搜索器。
pub struct TvSearch {
    wins: [Option<ViewWin>; 64],
    win_len: usize,
    /// 最近一次过滤耗时（ms，宿主注入——红线对账）。
    last_filter_ms: u64,
    /// 当前命中集（id 表——UI 高亮唯一源）。
    hits: [u64; 64],
    hit_len: usize,
}

impl TvSearch {
    pub fn new() -> TvSearch {
        TvSearch {
            wins: [(); 64].map(|_| None),
            win_len: 0,
            last_filter_ms: 0,
            hits: [0; 64],
            hit_len: 0,
        }
    }

    /// 注册窗口（F081 状态同步接缝——窗口开合由宿主推）。
    pub fn add_win(&mut self, id: u64, title: &'static str, app: &'static str, desk: usize) {
        if self.win_len < 64 {
            self.wins[self.win_len] = Some(ViewWin { id, title, app, desk });
            self.win_len += 1;
        }
    }

    /// 窗口关闭同步（F081）。
    pub fn remove_win(&mut self, id: u64) -> bool {
        for i in 0..self.win_len {
            if let Some(w) = &self.wins[i] {
                if w.id == id {
                    self.wins[i] = None;
                    // 压实。
                    let mut j = i;
                    while j + 1 < self.win_len {
                        self.wins.swap(j, j + 1);
                        j += 1;
                    }
                    self.win_len -= 1;
                    return true;
                }
            }
        }
        false
    }

    /// 桌面迁移同步（F235）。
    pub fn move_win(&mut self, id: u64, desk: usize) -> bool {
        for slot in self.wins[..self.win_len].iter_mut().flatten() {
            if slot.id == id {
                slot.desk = desk;
                return true;
            }
        }
        false
    }

    /// 过滤：标题/应用名子串匹配（大小写敏感——中英混排模型面统一小写由
    /// 宿主预处理注入；此处为逐字节子串）。
    ///
    /// 返回命中数；命中集存 hits（UI 高亮唯一源）。
    pub fn filter(&mut self, needle: &str, elapsed_ms: u64) -> usize {
        self.last_filter_ms = elapsed_ms;
        self.hit_len = 0;
        if needle.is_empty() {
            // 空查询 = 全量（视图默认态——不算命中，返回哨兵含义由 len 表达）。
            for slot in self.wins[..self.win_len].iter().flatten() {
                self.hits[self.hit_len] = slot.id;
                self.hit_len += 1;
            }
            return self.hit_len;
        }
        for slot in self.wins[..self.win_len].iter().flatten() {
            if slot.title.contains(needle) || slot.app.contains(needle) {
                self.hits[self.hit_len] = slot.id;
                self.hit_len += 1;
            }
        }
        self.hit_len
    }

    /// 过滤是否在线内（即时性红线对账）。
    pub fn within_budget(&self) -> bool {
        self.last_filter_ms <= FILTER_BUDGET_MS
    }

    /// 命中集快照（UI 高亮唯一源）。
    pub fn hit_ids(&self) -> &[u64] {
        &self.hits[..self.hit_len]
    }

    /// Enter 聚焦：命中的第 i 项 → 窗口 id（跨桌命中时由调用方先跳桌面）。
    pub fn focus_hit(&self, i: usize) -> Option<u64> {
        self.hits.get(i).copied()
    }

    /// 跨桌标注：命中项所在桌面（跳转预告——「标注在哪桌」）。
    pub fn desk_of_hit(&self, i: usize) -> Option<usize> {
        let id = self.focus_hit(i)?;
        self.wins[..self.win_len]
            .iter()
            .flatten()
            .find(|w| w.id == id)
            .map(|w| w.desk)
    }

    /// 空态出路：零命中时给出开新窗引导文案。
    pub fn empty_guidance(&self, hits: usize) -> Option<&'static str> {
        if hits == 0 {
            Some(EMPTY_GUIDANCE)
        } else {
            None
        }
    }
}

impl Default for TvSearch {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_tvsearch_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 过滤即时性：99ms 在线内、101ms 触线拒绝（红线对账）。
    let mut s = TvSearch::new();
    s.add_win(1, "报告 - 记事本", "记事本", 0);
    s.add_win(2, "终端", "终端", 1);
    s.filter("记事本", 99);
    let under = s.within_budget();
    s.filter("记事本", 101);
    set.add("filter budget 100ms", under && !s.within_budget(), "");

    // 2. 标题/应用名双面匹配。
    s.filter("记事本", 10);
    let by_title = s.hit_ids() == [1u64];
    s.filter("终端", 10);
    let by_app = s.hit_ids() == [2u64];
    set.add("match by title and app", by_title && by_app, "");

    // 3. 跨桌标注与跳转：跨桌命中给出所在桌面（F235 桌面号）。
    s.add_win(3, "浏览器", "浏览器", 2);
    s.filter("浏览", 10);
    set.add(
        "cross desk annotation",
        s.hit_ids() == [3u64] && s.desk_of_hit(0) == Some(2),
        "",
    );

    // 4. Enter 聚焦：命中第 i 项返回窗口 id（4 号「浏览器设置」按序第 2）。
    s.add_win(4, "浏览器设置", "浏览器", 0);
    s.filter("浏览器", 10);
    let focus = s.focus_hit(1);
    set.add("enter focuses hit", focus == Some(4), "");

    // 5. 空态出路：零命中给出开新窗引导。
    s.filter("不存在", 10);
    set.add(
        "empty state guidance",
        s.hit_ids().is_empty() && s.empty_guidance(0) == Some(EMPTY_GUIDANCE),
        "",
    );

    // 6. 状态同步：窗口关了搜不到；跨桌移动后标注跟新（F081/F235）。
    s.remove_win(4);
    s.move_win(3, 0);
    s.filter("浏览器", 10);
    set.add(
        "sync remove and desk move",
        s.hit_ids() == [3u64] && s.desk_of_hit(0) == Some(0),
        "",
    );

    // 7. 空查询 = 全量视图默认态（余窗 1/2/3 三枚）。
    s.filter("", 10);
    set.add("empty query shows all", s.hit_ids().len() == 3, "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_missing_window_false() {
        let mut s = TvSearch::new();
        assert!(!s.remove_win(9));
    }

    #[test]
    fn filter_no_hits_then_recover() {
        let mut s = TvSearch::new();
        s.add_win(1, "a", "a", 0);
        assert_eq!(s.filter("zz", 5), 0);
        assert_eq!(s.filter("a", 5), 1);
    }

    #[test]
    fn desk_of_hit_out_of_range_none() {
        let s = TvSearch::new();
        assert!(s.desk_of_hit(0).is_none());
    }
}
