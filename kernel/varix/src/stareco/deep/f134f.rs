//! 深化层三 · F134 主题分享页（2026-09-26 深化批次三）。
//!
//! 补深目录工程面（主册 G-D-09）：标签倒排索引（多标签交集检索）、
//! 审核队列排序器（SLA 余量升序——快到期的先审）、下载计数一致性
//! （分桶日账 vs 总账对拍）、评分聚合（半值向下取整防虚高）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 标签倒排索引：tag → id 列表；多标签查询取交集，结果按 id 升序
// ---------------------------------------------------------------------------

pub struct TagIndex {
    /// (标签, 主题 id) 登记序列。
    entries: alloc::vec::Vec<(&'static str, u32)>,
}

impl TagIndex {
    pub fn new() -> TagIndex {
        TagIndex { entries: alloc::vec::Vec::new() }
    }

    pub fn tag(&mut self, tag: &'static str, id: u32) {
        if !tag.is_empty() {
            self.entries.push((tag, id));
        }
    }

    fn ids_for(&self, tag: &str) -> alloc::vec::Vec<u32> {
        let mut v: alloc::vec::Vec<u32> =
            self.entries.iter().filter(|(t, _)| *t == tag).map(|(_, i)| *i).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// 多标签交集（空标签集 = 空结果，不返回全库——防误操作全量导出）。
    pub fn query(&self, tags: &[&'static str]) -> alloc::vec::Vec<u32> {
        if tags.is_empty() {
            return alloc::vec::Vec::new();
        }
        let first = self.ids_for(tags[0]);
        let mut out = first;
        for t in &tags[1..] {
            let ids = self.ids_for(t);
            out.retain(|i| ids.contains(i));
        }
        out
    }

    /// 标签膨胀防线：登记的互异标签数（词表固定 12 词的核对输入）。
    pub fn distinct_tags(&self) -> usize {
        let mut seen: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
        for (t, _) in &self.entries {
            if !seen.contains(t) {
                seen.push(t);
            }
        }
        seen.len()
    }
}

// ---------------------------------------------------------------------------
// 审核队列排序器：SLA 剩余天数升序（余量最小先审），平局按提交日早者
// ---------------------------------------------------------------------------

pub struct ReviewItem {
    pub id: u32,
    pub submitted_day: u32,
    /// SLA 到期日（提交 + 7 天窗口或已延长）。
    pub due_day: u32,
}

/// 审阅次序插入序比较：未到期优先；同态按（到期日, 提交日）小者先；
/// 同为已到期按提交日早者先。返回 b（待插入项）是否应排在 a 前。
fn review_before(a: &ReviewItem, b: &ReviewItem, today: u32) -> bool {
    let a_over = a.due_day <= today;
    let b_over = b.due_day <= today;
    match (a_over, b_over) {
        (true, false) => true,
        (false, true) => false,
        (false, false) => (b.due_day, b.submitted_day) < (a.due_day, a.submitted_day),
        (true, true) => b.submitted_day < a.submitted_day,
    }
}

/// 选择序输出审阅次序；到期日已过（余量 0）的沉底并标记。
pub fn review_order(items: &[ReviewItem], today: u32) -> alloc::vec::Vec<usize> {
    let mut idx: alloc::vec::Vec<usize> = (0..items.len()).collect();
    for i in 1..idx.len() {
        let key = idx[i];
        let mut j = i;
        while j > 0 && review_before(&items[idx[j - 1]], &items[key], today) {
            idx[j] = idx[j - 1];
            j -= 1;
        }
        idx[j] = key;
    }
    idx
}

// ---------------------------------------------------------------------------
// 下载计数：日分桶账 + 总账对拍（一处一事实的一致性机器面）
// ---------------------------------------------------------------------------

pub struct DownloadLedger {
    /// (日, 次数) 按日分桶。
    buckets: alloc::vec::Vec<(u32, u32)>,
    /// 总账计数。
    total: u32,
}

impl DownloadLedger {
    pub fn new() -> DownloadLedger {
        DownloadLedger { buckets: alloc::vec::Vec::new(), total: 0 }
    }

    pub fn hit(&mut self, day: u32) {
        self.total += 1;
        match self.buckets.iter_mut().find(|(d, _)| *d == day) {
            Some((_, c)) => *c += 1,
            None => self.buckets.push((day, 1)),
        }
    }

    /// 总账与分桶合计一致（漂移即红）。
    pub fn consistent(&self) -> bool {
        let sum: u32 = self.buckets.iter().map(|(_, c)| c).sum();
        sum == self.total
    }

    /// 最近 n 日（含 today 往回）热度序列——目录排序的数据源。
    pub fn window_counts(&self, today: u32, days: u32) -> alloc::vec::Vec<u32> {
        let mut out = alloc::vec::Vec::new();
        let mut d = today;
        let mut guard = 0;
        while guard < days {
            let c = self.buckets.iter().find(|(bd, _)| *bd == d).map(|(_, c)| *c).unwrap_or(0);
            out.push(c);
            if d == 0 {
                break;
            }
            d -= 1;
            guard += 1;
        }
        out
    }

    pub fn total(&self) -> u32 {
        self.total
    }
}

// ---------------------------------------------------------------------------
// 评分聚合：半值向下取整（4.5 → 4，防虚高）+ 评分数下限门槛
// ---------------------------------------------------------------------------

/// 输入评分序列（1..=5），返回 (均值×10 取整, 是否达展示门槛)。
/// 门槛：≥5 条有效评分才展示均值（小样本不误导）；越界值整体剔除
/// （分子分母同剔——脏数据不稀释均值）。
pub fn rating_summary(scores: &[u32]) -> (u32, bool) {
    let valid: alloc::vec::Vec<u32> =
        scores.iter().copied().filter(|s| *s >= 1 && *s <= 5).collect();
    if valid.is_empty() {
        return (0, false);
    }
    let sum: u32 = valid.iter().sum();
    let shown = valid.len() >= 5;
    let avg10 = sum * 10 / valid.len() as u32;
    // 半值向下：.5 尾数直接舍去（avg10 尾数 5 → 减 5 再除）。
    let rounded = if avg10 % 10 == 5 { avg10 - 5 } else { avg10 } / 10;
    (rounded, shown)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F134F_TAG: &str = "stareco-F134-deep3";

pub fn run_f134_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F134F_TAG);

    // 倒排索引
    let mut ti = TagIndex::new();
    ti.tag("dark", 1);
    ti.tag("minimal", 1);
    ti.tag("dark", 2);
    ti.tag("glass", 3);
    ti.tag("dark", 3);
    set.add("f134f single tag", ti.query(&["dark"]) == alloc::vec![1, 2, 3], "单标签全命中且有序");
    set.add(
        "f134f intersect",
        ti.query(&["dark", "glass"]) == alloc::vec![3],
        "双标签交集",
    );
    set.add("f134f empty query", ti.query(&[]).is_empty(), "空标签集返回空");
    set.add("f134f distinct", ti.distinct_tags() == 3, "互异标签计数");

    // 审核队列
    let items = [
        ReviewItem { id: 1, submitted_day: 100, due_day: 110 },
        ReviewItem { id: 2, submitted_day: 98, due_day: 105 },
        ReviewItem { id: 3, submitted_day: 90, due_day: 97 },
        ReviewItem { id: 4, submitted_day: 99, due_day: 106 },
    ];
    let order = review_order(&items, 100);
    set.add("f134f order first", items[order[0]].id == 2, "余量最小先审");
    set.add("f134f order second", items[order[1]].id == 4, "余量次小随后");
    set.add("f134f order third", items[order[2]].id == 1, "余量再次");
    set.add("f134f order overdue last", items[order[3]].id == 3, "已到期沉底");

    // 下载账本
    let mut dl = DownloadLedger::new();
    dl.hit(10);
    dl.hit(10);
    dl.hit(11);
    set.add("f134f dl consistent", dl.consistent() && dl.total() == 3, "总账=分桶合计");
    set.add("f134f dl window", dl.window_counts(11, 2) == alloc::vec![1, 2], "窗口序列当日在前");
    set.add("f134f dl gap", dl.window_counts(13, 3) == alloc::vec![0, 0, 1], "空日补零");

    // 评分
    set.add("f134f rating gate", rating_summary(&[5, 4]).1 == false, "小样本不展示");
    set.add("f134f rating half down", rating_summary(&[5, 4, 4, 4, 5]).0 == 4, "4.4→4；4.5 舍去");
    set.add("f134f rating five", rating_summary(&[5, 5, 5, 5, 5]).0 == 5, "满分五分");
    set.add("f134f rating empty", rating_summary(&[]) == (0, false), "空输入哨兵");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn tag_index_intersection_order() {
        let mut ti = TagIndex::new();
        ti.tag("a", 5);
        ti.tag("b", 5);
        ti.tag("a", 2);
        ti.tag("a", 2); // 重复登记去重——同一 (tag,id) 只算一次
        let q = ti.query(&["a"]);
        assert_eq!(q, alloc::vec![2, 5]);
        let inter = ti.query(&["a", "b"]);
        assert_eq!(inter, alloc::vec![5]);
    }

    #[test]
    fn rating_boundaries() {
        assert_eq!(rating_summary(&[1, 1, 1, 1, 1]).0, 1);
        assert_eq!(rating_summary(&[2, 3, 3, 3, 3]).0, 2); // 2.8 → 2
        // 越界值分子分母同剔：有效 5 条 (5×5)/5=5
        let (v, ok) = rating_summary(&[0, 9, 5, 5, 5, 5, 5]);
        assert!(ok);
        assert_eq!(v, 5);
        // 全越界 → 空哨兵
        assert_eq!(rating_summary(&[0, 9]), (0, false));
    }
}
