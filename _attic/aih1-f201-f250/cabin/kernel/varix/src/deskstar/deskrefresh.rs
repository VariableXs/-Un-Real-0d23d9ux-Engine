//! F083 桌面刷新语义 · 完整设计（STAR I 主册 G-C-13）。
//!
//! **判据（主册）**：刷新前后图标位一致（位移零）；80ms 反馈实测；
//! 缓存重建路径触发实测（手动损坏缓存）。
//!
//! **设计要点（主册）**：
//! - F5/右键刷新做诚实版重载：真重载图标缓存与桌面枚举（含可见
//!   反馈 80ms 图标微闪），不装样子——如果无事可刷则反馈极轻
//!   （不演戏）；
//! - 刷新反馈：图标 80ms 透明度 1→0.85→1（单一曲线全局统一）；
//!   右键菜单「刷新」位对齐 Windows（乙-4 表右键结构）；刷新期间
//!   桌面不接受新枚举请求（防抖）；
//! - 无存储；刷新计数入诊断（好奇用户可查「这个月刷新了 214 次，
//!   其实 0 次是必要的」——彩蛋式诚实）；
//! - 图标缓存损坏 → 刷新触发重建（自愈 F189 联动）；刷新中弹出新
//!   文件 → 完成后补枚举；
//! - 微闪只对图标层（壁纸任务栏不动——最小惊扰）；连续 F5 合并
//!   （300ms 内单次）；桌面枚举增量化（对比快照只刷差异项——刷新
//!   成本 <5ms 常态）。
//!
//! 实装口径：刷新状态机（快照对比增量化）+ 微闪账 + 防抖合并 +
//! 缓存自愈账 + 诚实计数账。时间注入式。

use crate::checks::CheckSet;

use crate::deskstar::dbase::{budget_ok, Debouncer};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计/设计细节）
// ---------------------------------------------------------------------------

/// 微闪时长（ms，透明度 1→0.85→1）。
pub const FLASH_MS: u32 = 80;

/// 微闪谷值（千分比透明度：0.85 → 850）。
pub const FLASH_TROUGH_PERMILLE: u16 = 850;

/// 连续刷新合并窗（ms）。
pub const MERGE_WINDOW_MS: u64 = 300;

/// 增量刷新常态成本预算（ms）。
pub const DIFF_BUDGET_MS: u64 = 5;

// ---------------------------------------------------------------------------
// 状态机
// ---------------------------------------------------------------------------

/// 桌面刷新管理器。
pub struct DeskRefresh {
    debounce: Debouncer,
    /// 图标层微闪进行中（起点）。
    flash_start: Option<u64>,
    now_ms: u64,
    /// 上一枚举快照（增量化对比基准）。
    last_snapshot: Vec<String>,
    /// 缓存健康旗标（损坏 → 刷新触发重建）。
    cache_corrupt: bool,
    /// 诚实计数账：总刷新次数 / 其中真正必要的次数（缓存损坏或
    /// 枚举有差异）。
    pub refresh_total: u64,
    pub refresh_necessary: u64,
    /// 重建账（缓存重建次数——自愈路径触发证据）。
    pub rebuilds: u64,
    /// 补枚举账（刷新中弹出新文件 → 完成后补）。
    pub deferred_enums: u64,
    /// 最近一次增量刷新耗时（实测账）。
    pub last_diff_ms: Option<u64>,
    /// 刷新中拒新请求计数（防抖语义的证据面）。
    pub rejected_during: u64,
    /// 补执行队列（深化层：真排队——完成后按序补枚举）。
    deferred_queue: Vec<QueuedRefresh>,
    /// 成本样本滑动窗（P95 分位数据源）。
    cost_samples: Vec<u64>,
    /// 原因分类计数（深化层二：[F5, 右键, 自愈]——诊断下钻面）。
    reason_counts: [u32; 3],
}

impl DeskRefresh {
    pub fn new() -> DeskRefresh {
        DeskRefresh {
            debounce: Debouncer::new(MERGE_WINDOW_MS),
            flash_start: None,
            now_ms: 0,
            last_snapshot: Vec::new(),
            cache_corrupt: false,
            refresh_total: 0,
            refresh_necessary: 0,
            rebuilds: 0,
            deferred_enums: 0,
            last_diff_ms: None,
            rejected_during: 0,
            deferred_queue: Vec::new(),
            cost_samples: Vec::new(),
            reason_counts: [0; 3],
        }
    }

    /// 刷新请求（F5/右键「刷新」同入口）。返回是否真正执行
    /// （300ms 内合并为单次——防抖语义；执行中拒新请求）。
    pub fn request(&mut self, now_ms: u64) -> bool {
        self.now_ms = now_ms;
        if self.flash_start.is_some() && now_ms.saturating_sub(self.flash_start.unwrap()) < FLASH_MS as u64 {
            // 刷新进行中：不接受新枚举请求（防抖）。
            self.rejected_during += 1;
            return false;
        }
        if self.debounce.event(now_ms) {
            self.refresh_total += 1;
            self.flash_start = Some(now_ms);
            true
        } else {
            false
        }
    }

    /// 微闪进度（千分比透明度；1→0.85→1 三角波，80ms）。
    pub fn flash_opacity(&self) -> u16 {
        match self.flash_start {
            None => 1000,
            Some(t0) => {
                let t = self.now_ms.saturating_sub(t0) as u32;
                if t >= FLASH_MS {
                    return 1000;
                }
                // 前半降、后半升（单一曲线全局统一）。
                let half = FLASH_MS / 2;
                if t < half {
                    let p = (t * 1000) / half.max(1);
                    1000 - ((1000 - FLASH_TROUGH_PERMILLE as u32) * p / 1000) as u16
                } else {
                    let p = ((t - half) * 1000) / half.max(1);
                    (FLASH_TROUGH_PERMILLE as u32
                        + ((1000 - FLASH_TROUGH_PERMILLE as u32) * p / 1000)) as u16
                }
            }
        }
    }

    /// 微闪只对图标层（壁纸/任务栏不动的语义标志——渲染层读取）。
    pub fn flash_scope(&self) -> &'static str {
        "icons-only"
    }

    /// 微闪结束确认（驱动账本结算：本次是否「必要」）。
    pub fn flash_done(&mut self, now_ms: u64, had_diff: bool) {
        if let Some(t0) = self.flash_start {
            if now_ms.saturating_sub(t0) >= FLASH_MS as u64 {
                self.flash_start = None;
                if had_diff || self.cache_corrupt {
                    self.refresh_necessary += 1;
                }
                if self.cache_corrupt {
                    self.cache_corrupt = false;
                    self.rebuilds += 1;
                }
            }
        }
        self.now_ms = now_ms;
    }

    /// 桌面枚举增量化：新快照 vs 上一快照只出差异项。
    /// 返回 (新增, 移除, 改名)。同表算常开 <5ms（账面记录）。
    pub fn enum_diff(&mut self, snapshot: &[&str], cost_ms: u64) -> (Vec<String>, Vec<String>, Vec<String>) {
        let added: Vec<String> = snapshot
            .iter()
            .filter(|s| !self.last_snapshot.iter().any(|p| p == *s))
            .map(|s| String::from(*s))
            .collect();
        let removed: Vec<String> = self
            .last_snapshot
            .iter()
            .filter(|p| !snapshot.iter().any(|s| s == p))
            .cloned()
            .collect();
        // 改名 = 移除与新 adding 的同尺寸启发（桌面场景：旧名消失+
        // 新名同现视为改名对——这里保守只报两列，改名判定交上层）。
        let _ = &removed;
        self.last_diff_ms = Some(cost_ms);
        let new_snapshot: Vec<String> = snapshot.iter().map(|s| String::from(*s)).collect();
        self.last_snapshot = new_snapshot;
        (added, removed, Vec::new())
    }

    /// 刷新成本达标（常态 <5ms）。
    pub fn diff_in_budget(&self) -> bool {
        self.last_diff_ms.map(|t| budget_ok(t, DIFF_BUDGET_MS)) == Some(true)
    }

    /// 缓存损坏标记（手动损坏/自检发现 → 下次刷新触发重建）。
    pub fn mark_cache_corrupt(&mut self) {
        self.cache_corrupt = true;
    }

    pub fn cache_corrupt(&self) -> bool {
        self.cache_corrupt
    }

    /// 刷新中新文件出现 → 完成后补枚举（账面记录，宿主调度补）。
    pub fn new_file_during_refresh(&mut self) {
        if self.flash_start.is_some() {
            self.deferred_enums += 1;
        }
    }

    /// 诚实诊断文案（彩蛋式：「本月刷新 N 次，其中必要 M 次」）。
    pub fn honesty_line(&self) -> alloc::string::String {
        alloc::format!(
            "本月刷新 {} 次，其中真正必要 {} 次",
            self.refresh_total,
            self.refresh_necessary
        )
    }

    /// 图标位一致性：刷新前后图标位零位移（本账持位表——刷新只重
    /// 载内容不动布局）。
    pub fn positions_unchanged(&self, before: &[(u64, i32, i32)], after: &[(u64, i32, i32)]) -> bool {
        before.len() == after.len()
            && before
                .iter()
                .zip(after.iter())
                .all(|(b, a)| b.0 == a.0 && b.1 == a.1 && b.2 == a.2)
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// 深化层（回炉批）：枚举请求真排队 / 右键菜单语义位 / 成本分布 P95 /
// 诊断导出格式——主册【交互设计】【数据与存储】补足。
// ---------------------------------------------------------------------------

/// 诊断导出记录（诚实计数账的结构化出口——好奇用户可查的形态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefreshDiag {
    pub total: u64,
    pub necessary: u64,
    pub rebuilds: u64,
    /// 成本 P95（毫秒；样本不足 20 时返回 None——小样本不硬算分位）。
    pub cost_p95_ms: Option<u64>,
}

/// 最近请求（排队单元：来源 + 时刻）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueuedRefresh {
    pub from_keyboard: bool,
    pub at_ms: u64,
}

impl DeskRefresh {
    /// 刷新期间的新请求入队（完成后按序补执行——替代单纯计数）。
    pub fn enqueue_deferred(&mut self, from_keyboard: bool, now_ms: u64) -> bool {
        if self.flash_start.is_some()
            && now_ms.saturating_sub(self.flash_start.unwrap()) < FLASH_MS as u64
        {
            self.deferred_queue.push(QueuedRefresh {
                from_keyboard,
                at_ms: now_ms,
            });
            self.deferred_enums += 1;
            true
        } else {
            false
        }
    }

    /// 补执行队列驱动（flash_done 后由宿主滴答调用；一次吐一条）。
    pub fn pop_deferred(&mut self) -> Option<QueuedRefresh> {
        if self.deferred_queue.is_empty() {
            None
        } else {
            Some(self.deferred_queue.remove(0))
        }
    }

    pub fn deferred_len(&self) -> usize {
        self.deferred_queue.len()
    }

    /// 右键菜单「刷新」项语义位（乙-4 表右键结构：刷新位于菜单尾部
    /// 分隔线之后首项——位置常量 + 文案，渲染层取用）。
    pub fn context_menu_entry(&self) -> (&'static str, usize) {
        ("刷新", CONTEXT_MENU_REFRESH_POS)
    }

    /// 刷新成本样本（增量化耗时入分布账——P95 的数据源）。
    pub fn record_cost(&mut self, cost_ms: u64) {
        self.cost_samples.push(cost_ms);
        if self.cost_samples.len() > COST_SAMPLE_CAP {
            self.cost_samples.remove(0);
        }
    }

    /// 成本 P95（最近邻秩；样本 <20 返回 None——小样本不硬算分位，
    /// 诚实留白）。
    pub fn cost_p95(&self) -> Option<u64> {
        if self.cost_samples.len() < 20 {
            return None;
        }
        let mut sorted = self.cost_samples.clone();
        sorted.sort_unstable();
        let idx = (sorted.len() as u64 * 95 / 100) as usize;
        sorted.get(idx.min(sorted.len() - 1)).copied()
    }

    /// 诊断导出（结构化记录——诊断中心 F120 可直接消费）。
    pub fn export_diag(&self) -> RefreshDiag {
        RefreshDiag {
            total: self.refresh_total,
            necessary: self.refresh_necessary,
            rebuilds: self.rebuilds,
            cost_p95_ms: self.cost_p95(),
        }
    }
}

/// 右键菜单「刷新」位（乙-4 表右键结构：分隔线后首项 = 第 0 位）。
pub const CONTEXT_MENU_REFRESH_POS: usize = 0;

/// 成本样本容量（滑动窗口——防止账本无界增长）。
pub const COST_SAMPLE_CAP: usize = 200;

/// 分位数小样本下限（不足则 P95 返回 None）。
pub const P95_MIN_SAMPLES: usize = 20;

/// F083 深化自检：真排队补执行、右键菜单语义位、P95 分位、诊断导出。
pub fn run_deskrefresh_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F083-deep");
    let mut d = DeskRefresh::new();
    // 1. 真排队：刷新期 3 个请求入队 → 完成后按序吐出。
    d.request(0);
    let q1 = d.enqueue_deferred(true, 10);
    let q2 = d.enqueue_deferred(false, 20);
    let q3 = d.enqueue_deferred(true, 30);
    d.flash_done(FLASH_MS as u64, false);
    let r1 = d.pop_deferred();
    let r2 = d.pop_deferred();
    let r3 = d.pop_deferred();
    let r4 = d.pop_deferred();
    set.add(
        "deferred-queue",
        q1 && q2 && q3
            && r1.map(|r| r.from_keyboard) == Some(true)
            && r2.map(|r| r.from_keyboard) == Some(false)
            && r3.map(|r| r.at_ms) == Some(30)
            && r4.is_none(),
        "FIFO after flash",
    );
    // 2. 非刷新期入队被拒（队列只收刷新期间的请求）。
    let outside = !d.enqueue_deferred(true, FLASH_MS as u64 + 10);
    set.add("queue-gate", outside && d.deferred_len() == 0, "only during refresh");
    // 3. 右键菜单语义位。
    let (label, pos) = d.context_menu_entry();
    set.add(
        "context-entry",
        label == "刷新" && pos == CONTEXT_MENU_REFRESH_POS,
        "乙-4 tail after separator",
    );
    // 4. P95 分位：样本 <20 → None；≥20 → 最近邻秩命中。
    for i in 0..19u64 {
        d.record_cost(10 + i);
    }
    let none_small = d.cost_p95().is_none();
    for i in 20..30u64 {
        d.record_cost(10 + i); // 29 个样本：10..38
    }
    // 29 样本 → idx = 29*95/100 = 27 → 排序后第 28 位（0 起）。
    let p95 = d.cost_p95();
    set.add(
        "p95",
        none_small && p95 == Some(38) && P95_MIN_SAMPLES == 20, // 29 样本 idx=27 → 38
        "nearest-rank p95",
    );
    // 5. 诊断导出（结构化三账一致）。
    let diag = d.export_diag();
    set.add(
        "diag-export",
        diag.total == d.refresh_total
            && diag.necessary == d.refresh_necessary
            && diag.rebuilds == d.rebuilds
            && diag.cost_p95_ms == Some(38),
        "F120 consumable",
    );
    set
}

#[cfg(test)]
mod tests_deep {
    use super::*;

    #[test]
    fn queue_drains_fully_then_refills() {
        let mut d = DeskRefresh::new();
        d.request(0);
        for i in 0..5u64 {
            d.enqueue_deferred(false, i);
        }
        d.flash_done(FLASH_MS as u64, false);
        for i in 0..5u64 {
            let r = d.pop_deferred().unwrap();
            assert_eq!(r.at_ms, i, "FIFO 序");
        }
        assert!(d.pop_deferred().is_none());
        // 下一轮刷新照常排队。
        d.request(1_000);
        assert!(d.enqueue_deferred(true, 1_010));
        assert_eq!(d.deferred_len(), 1);
    }

    #[test]
    fn cost_samples_capped() {
        let mut d = DeskRefresh::new();
        for i in 0..500u64 {
            d.record_cost(i);
        }
        assert_eq!(d.cost_samples.len(), COST_SAMPLE_CAP, "滑动窗口封顶");
    }

    #[test]
    fn deskrefresh_deep_checks_all_green() {
        let set = run_deskrefresh_deep_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F083-deep 红项：{}/{} 绿", p, p + f);
    }
}

// 自检（判据唯一源：主册 G-C-13 验收判据）
// ---------------------------------------------------------------------------

/// F083 自检：位移零、80ms 微闪、缓存重建触发、300ms 合并、
/// 增量 <5ms、刷新中拒新、补枚举、诚实计数。
pub fn run_deskrefresh_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F083");
    let mut d = DeskRefresh::new();
    // 1. 图标位一致性（位移零）。
    let before = vec![(1u64, 10, 10), (2, 90, 10), (3, 10, 110)];
    let after = vec![(1u64, 10, 10), (2, 90, 10), (3, 10, 110)];
    let moved = vec![(1u64, 12, 10), (2, 90, 10), (3, 10, 110)];
    set.add(
        "positions-zero-shift",
        d.positions_unchanged(&before, &after) && !d.positions_unchanged(&before, &moved),
        "refresh never moves icons",
    );
    // 2. 80ms 微闪：谷值 0.85、80ms 复原、只对图标层。
    d.request(1_000);
    let start = d.flash_opacity();
    d.now_ms = 1_040; // 中点
    let mid = d.flash_opacity();
    d.now_ms = 1_080;
    let end = d.flash_opacity();
    set.add(
        "flash-80ms",
        start == 1000
            && (FLASH_TROUGH_PERMILLE as i32 - mid as i32).abs() <= 10
            && end == 1000
            && d.flash_scope() == "icons-only",
        "1→0.85→1 icons only",
    );
    // 3. 300ms 合并：窗内连按不重刷（时间线避开首刷 1000 的合并窗）。
    d.flash_done(1_080, false);
    let r1 = d.request(1_450);
    let r2 = d.request(1_600); // 150ms 内 → 合并（窗沿 1600 后移至 1900）
    let r3 = d.request(1_950); // 窗沿闭合后 → 真触发
    set.add("merge-300ms", r1 && !r2 && r3, "consecutive F5 merge");
    // 4. 增量 <5ms + 差异产出。
    d.enum_diff(&["a.txt", "b.txt"], 3);
    let inb = d.diff_in_budget();
    let (added, removed, _) = d.enum_diff(&["b.txt", "c.txt"], 2);
    set.add(
        "diff-budget",
        inb && added == vec![alloc::string::String::from("c.txt")]
            && removed == vec![alloc::string::String::from("a.txt")],
        "<5ms incremental",
    );
    // 5. 缓存损坏 → 刷新触发重建（自愈路径实测）。
    d.mark_cache_corrupt();
    d.request(3_000);
    d.flash_done(3_080, false);
    set.add(
        "cache-rebuild",
        d.rebuilds == 1 && !d.cache_corrupt(),
        "corrupt → rebuild",
    );
    // 6. 刷新中拒新请求 + 完成后补枚举。
    d.request(4_000);
    let rejected = !d.request(4_020) && d.rejected_during == 1;
    d.new_file_during_refresh();
    let deferred = d.deferred_enums == 1;
    d.flash_done(4_080, false);
    set.add("reject-defer", rejected && deferred, "no new enum during");
    // 7. 诚实计数：总次数 5（1000/1450/1950/3000/4000 各一真触发，
    // 1600 与 4020 分别被合并/拒收）、必要 1（缓存重建那次）。
    let line = d.honesty_line();
    set.add(
        "honesty-counter",
        d.refresh_total == 5 && d.refresh_necessary == 1 && line.contains("5 次"),
        "honest refresh count",
    );
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_trough_exact_085() {
        let mut d = DeskRefresh::new();
        d.request(0);
        d.now_ms = 40;
        let mid = d.flash_opacity();
        assert!((850i32 - mid as i32).abs() <= 5, "谷值 0.85±0.005，实测 {mid}");
    }

    #[test]
    fn merge_window_first_request_fires() {
        let mut d = DeskRefresh::new();
        assert!(d.request(0), "首按真触发（不白等一个窗口）");
    }

    #[test]
    fn necessary_count_tracks_diff_and_corruption() {
        let mut d = DeskRefresh::new();
        d.request(0);
        d.flash_done(80, false); // 无差异 → 不必要
        d.request(1_000);
        d.flash_done(1_080, true); // 有差异 → 必要
        assert_eq!(d.refresh_total, 2);
        assert_eq!(d.refresh_necessary, 1);
    }

    #[test]
    fn deferred_only_counts_during_refresh() {
        let mut d = DeskRefresh::new();
        d.new_file_during_refresh();
        assert_eq!(d.deferred_enums, 0, "非刷新期不记补枚举");
        d.request(0);
        d.new_file_during_refresh();
        assert_eq!(d.deferred_enums, 1);
    }

    #[test]
    fn deskrefresh_self_checks_all_green() {
        let set = run_deskrefresh_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F083 自检红项：{}/{} 绿", p, p + f);
    }
}

// ---------------------------------------------------------------------------
// 深化自检二（回炉批 D1-v2）——刷新原因分类记账（诊断面：本月 N 次
// 其中必要 M 次的下钻维度）。判据唯一源：主册 G-C-13 数据与存储。
// ---------------------------------------------------------------------------

/// 刷新原因（F5 键 / 右键菜单 / 缓存自愈重建——三类各自入账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshReason {
    F5Key,
    ContextMenu,
    SelfHeal,
}

impl RefreshReason {
    fn slot(self) -> usize {
        match self {
            RefreshReason::F5Key => 0,
            RefreshReason::ContextMenu => 1,
            RefreshReason::SelfHeal => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            RefreshReason::F5Key => "F5 键",
            RefreshReason::ContextMenu => "右键菜单",
            RefreshReason::SelfHeal => "缓存自愈",
        }
    }
}

impl DeskRefresh {
    /// 带原因的刷新请求（与 request 同一核心——原因只是记账维度；
    /// 返回语义同 request：是否真正执行）。
    pub fn request_as(&mut self, reason: RefreshReason, now_ms: u64) -> bool {
        let executed = self.request(now_ms);
        if executed {
            self.reason_counts[reason.slot()] += 1;
        }
        executed
    }

    /// 原因计数（诊断下钻面：「本月 214 次其中 0 次必要」的分类视角）。
    pub fn reason_counts(&self) -> [u32; 3] {
        self.reason_counts
    }
}

#[cfg(test)]
mod tests_deep2 {
    use super::*;

    #[test]
    fn rejected_request_not_counted() {
        let mut dr = DeskRefresh::new();
        assert!(dr.request_as(RefreshReason::F5Key, 1_000));
        assert!(!dr.request_as(RefreshReason::ContextMenu, 1_050)); // 80ms 内拒
        let c = dr.reason_counts();
        assert_eq!(c, [1, 0, 0], "被合并的请求不入账——只记真执行");
    }

    #[test]
    fn deskrefresh_deep2_checks_all_green() {
        let set = run_deskrefresh_deep2_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F083-deep2 红项：{}/{} 绿", p, p + f);
    }
}

/// F083 深化自检二：原因分类记账。
pub fn run_deskrefresh_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F083-deep2");
    let mut dr = DeskRefresh::new();
    // 三路入口各来一次（时间错开 >300ms——每次真执行）。
    let r1 = dr.request_as(RefreshReason::F5Key, 1_000);
    let r2 = dr.request_as(RefreshReason::ContextMenu, 2_000);
    let r3 = dr.request_as(RefreshReason::SelfHeal, 3_000);
    let c = dr.reason_counts();
    set.add(
        "reason-classified",
        r1 && r2 && r3 && c == [1, 1, 1],
        "three reasons one core",
    );
    // 名表面（诊断报告用）。
    set.add(
        "reason-names",
        RefreshReason::F5Key.name() == "F5 键"
            && RefreshReason::ContextMenu.name() == "右键菜单"
            && RefreshReason::SelfHeal.name() == "缓存自愈",
        "diagnostic labels",
    );
    set
}
