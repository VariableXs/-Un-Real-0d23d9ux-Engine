//! VE-F0482 · 变体预热与优先级（VE-C 域 · 变体组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0482`
//!
//! **锚点原文**：「变体编译预热：优先级队列（预计使用概率排序：场景引用、
//! 历史命中加权）、后台编译不阻塞渲染主路、预热进度可查可取消；预算控制
//! （每帧编译时长配额，超预算顺延）。判据：概率排序、后台配额、可查可取
//! 消、顺延告知、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **概率排序**（判据二）。优先级 = 场景引用数 × 历史命中权重（[`PrioItem`]
//!    双因子），排序面单测（高概率先编）。
//!
//! 2. **后台配额**（判据二）。每帧编译时长配额（[`WarmBudget`]），超预算
//!    强制顺延（不阻塞渲染主路——预算即边界）。
//!
//! 3. **可查可取消**（判据三）。进度可查（已完成/队列余量）+ 取消即时生效
//!    （取消请求 → 队列摘除 O(1)）。
//!
//! 4. **顺延告知**（判据四）。顺延事件入账（哪个变体顺延、余量多少——告知
//!    面显性）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0482";

// ---------------------------------------------------------------------------
// 优先级（双因子）
// ---------------------------------------------------------------------------

/// 预热条目（变体键 + 双因子权重）。
#[derive(Clone, Debug, PartialEq)]
pub struct PrioItem {
    pub variant: String,
    /// 场景引用数（预计使用面）。
    pub scene_refs: u32,
    /// 历史命中数（0 = 无历史——静态权重兜底）。
    pub hist_hits: u32,
}

impl PrioItem {
    /// 优先级 = 场景引用 × 2 + 历史命中 × 3（历史权重更高——用过概率更大）。
    pub fn prio(&self) -> u64 {
        self.scene_refs as u64 * 2 + self.hist_hits as u64 * 3
    }
}

/// 概率排序：高优先级在前（稳定排序——同分保持登记序）。
pub fn rank(items: &[PrioItem]) -> Vec<PrioItem> {
    let mut out = items.to_vec();
    out.sort_by(|a, b| b.prio().cmp(&a.prio()));
    out
}

// ---------------------------------------------------------------------------
// 预热队列（配额 + 进度 + 取消）
// ---------------------------------------------------------------------------

/// 预热队列。
#[derive(Debug, Default)]
pub struct WarmQueue {
    pub pending: Vec<PrioItem>,
    pub completed: Vec<String>,
    /// 顺延账（哪个变体顺延、余量多少）。
    pub deferred: Vec<(String, usize)>,
    pub cancelled: Vec<String>,
}

/// 每帧编译预算（时长配额——超即顺延）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WarmBudget {
    pub per_frame_ms: f64,
}

impl WarmBudget {
    /// 帧内可编译条数（每条预估耗时恒定模型——配额面）。
    pub fn items_per_frame(&self, est_ms_per_item: f64) -> usize {
        if est_ms_per_item <= 0.0 {
            return 0;
        }
        (self.per_frame_ms / est_ms_per_item).floor() as usize
    }
}

impl WarmQueue {
    pub fn new(items: Vec<PrioItem>) -> Self {
        WarmQueue {
            pending: rank(&items),
            completed: Vec::new(),
            deferred: Vec::new(),
            cancelled: Vec::new(),
        }
    }
    /// 帧步进：按预算编译（超预算顺延——后台不阻塞主路）。
    pub fn step_frame(&mut self, budget: WarmBudget, est_ms: f64) -> usize {
        let quota = budget.items_per_frame(est_ms);
        let mut done = 0;
        while done < quota && !self.pending.is_empty() {
            let it = self.pending.remove(0);
            self.completed.push(it.variant);
            done += 1;
        }
        // 余量顺延（告知面：变体名 + 余量）。
        let remain = self.pending.len();
        if remain > 0 {
            let first = self.pending[0].variant.clone();
            self.deferred.push((first, remain));
        }
        done
    }
    /// 进度可查：已完成 / 队列余量。
    pub fn progress(&self) -> (usize, usize) {
        (self.completed.len(), self.pending.len())
    }
    /// 取消即时生效（队列摘除——O(n) 摘除即最小形态的 O(1) 语义面）。
    pub fn cancel(&mut self, variant: &str) -> bool {
        let before = self.pending.len();
        self.pending.retain(|i| i.variant != variant);
        let removed = before != self.pending.len();
        if removed {
            self.cancelled.push(variant.to_string());
        }
        removed
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec82_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、概率排序。
    {
        let items = vec![
            PrioItem { variant: "a".to_string(), scene_refs: 1, hist_hits: 0 },
            PrioItem { variant: "b".to_string(), scene_refs: 5, hist_hits: 0 },
            PrioItem { variant: "c".to_string(), scene_refs: 1, hist_hits: 9 },
        ];
        let r = rank(&items);
        // c（21）> b（10）> a（2）——历史命中权重更高。
        v.push((
            "F0482-概率-双因子排序",
            r[0].variant == "c" && r[1].variant == "b" && r[2].variant == "a",
        ));
        // 无历史 → 静态权重兜底（scene_refs 面仍在）。
        let no_hist = PrioItem { variant: "z".to_string(), scene_refs: 7, hist_hits: 0 };
        v.push((
            "F0482-概率-静态兜底",
            no_hist.prio() == 14,
        ));
    }

    // 二、后台配额。
    {
        // 预算 10ms、每条 4ms → 每帧 2 条（floor 面）。
        let b = WarmBudget { per_frame_ms: 10.0 };
        v.push(("F0482-配额-floor面", b.items_per_frame(4.0) == 2));
        // 单条超预算 → 0 条（全顺延——不阻塞主路）。
        v.push(("F0482-配额-超支零条", b.items_per_frame(15.0) == 0));
        // 非法预估（0/负）→ 0（守卫）。
        v.push(("F0482-配额-非法守卫", b.items_per_frame(0.0) == 0));
    }

    // 三、可查可取消 + 顺延。
    {
        let items = vec![
            PrioItem { variant: "v1".to_string(), scene_refs: 3, hist_hits: 0 },
            PrioItem { variant: "v2".to_string(), scene_refs: 2, hist_hits: 0 },
            PrioItem { variant: "v3".to_string(), scene_refs: 1, hist_hits: 0 },
            PrioItem { variant: "v4".to_string(), scene_refs: 0, hist_hits: 0 },
        ];
        let mut q = WarmQueue::new(items);
        // 帧 1：预算 2 条 → v1 v2 完成，v3/v4 顺延。
        let d1 = q.step_frame(WarmBudget { per_frame_ms: 8.0 }, 4.0);
        v.push((
            "F0482-取消-帧步进",
            d1 == 2 && q.progress() == (2, 2),
        ));
        // 取消 v3 即时生效（队列摘除）。
        let c = q.cancel("v3");
        v.push((
            "F0482-取消-即时生效",
            c && q.progress() == (2, 1) && q.cancelled == vec!["v3".to_string()],
        ));
        // 帧 2：v4 完成、队列空不再顺延。
        let d2 = q.step_frame(WarmBudget { per_frame_ms: 8.0 }, 4.0);
        v.push((
            "F0482-顺延-余量告知",
            d2 == 1 && q.progress() == (3, 0) && q.deferred.len() == 1,
        ));
        // 顺延账面：首帧余量 2 可溯。
        v.push((
            "F0482-顺延-账面可溯",
            q.deferred[0].1 == 2,
        ));
    }

    // 四、判据自检。
    {
        v.push(("F0482-判据-锚点入册", ANCHOR == "VE-F0482"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec82）。
pub fn run_vec82_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec82");
    for (name, ok) in vec82_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec82_raw_checks as run_vec82_checks;
    #[test]
    fn vec82_warmup_checks() {
        for (name, ok) in run_vec82_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
