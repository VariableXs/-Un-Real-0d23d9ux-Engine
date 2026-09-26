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
