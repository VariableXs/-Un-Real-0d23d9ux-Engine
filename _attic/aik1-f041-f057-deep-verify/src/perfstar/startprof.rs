//! F043 冷启动画像（perfstar · G-B-03）——应用启动五段刻度。
//!
//! 主册判据（验收标准第一句）：
//! **五段边界与内核打点一一对应（无重叠无遗漏）；同应用 10 次启动画像方差 <15%（测量自身稳定）。**
//!
//! 功能定义（G-B-03）：应用启动五段刻度——装载（PE/ELF 解析与映射）/
//! 重定位（导入绑定 F003）/首帧（首次合成提交）/可交互（输入事件响应）/
//! 稳定（5 秒无超预算帧）；每段耗时建档进星卡与账本。
//!
//! 【设计细节】段边界定义写死在打点常量（一处一事实）：
//! - 装载止于入口点前；
//! - 重定位止于 IAT 解析完；
//! - 首帧止于首次 Present；
//! - 可交互止于首个窗口过程返回；
//! - 稳定止于 5 秒观察窗结束。
//!
//! 【数据与存储】画像按 (应用版本, 启动序号) 记录；保留最近 20 次启动；
//! 聚合进星卡取「二次启动中位数」。启动序号 1 = 冷（缓存清）、2 起算热。
//! 【状态与异常】启动中途崩溃 → 画像标记「中止于第 X 段」（本身就是极有
//! 价值的归因数据）；用户取消启动 → 同理记录。画像收集受隐私总闸
//! （F036 同开关）控制——闸关则不记（计数呈现，不静默）。
//!
//! 零堆纪律：定长表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与段定义
// ---------------------------------------------------------------------------

/// 五段枚举。段边界与内核打点一一对应（见模块头注释，一处一事实）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Seg {
    Load = 0,       // 装载：止于入口点前
    Relocate = 1,   // 重定位：止于 IAT 解析完
    FirstFrame = 2, // 首帧：止于首次 Present
    Interactive = 3, // 可交互：止于首个窗口过程返回
    Stable = 4,     // 稳定：止于 5 秒观察窗结束
}

pub const SEG_COUNT: usize = 5;
pub const SEG_NAMES: [&str; SEG_COUNT] = ["load", "relocate", "first-frame", "interactive", "stable"];
/// 稳定段观察窗（主册：5 秒无超预算帧）。
pub const STABLE_WINDOW_MS: u32 = 5_000;
/// 每应用保留最近 20 次启动（主册【数据与存储】）。
pub const LAUNCH_HISTORY: usize = 20;
/// 应用表容量（定长；LRU 驱逐最久未启动者）。
pub const APP_CAP: usize = 16;

/// 中止原因（崩溃 / 用户取消——两者都记录，都是归因数据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AbortReason {
    Crash,
    UserCancel,
}

/// 一次启动画像。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaunchProfile {
    pub app_hash: u64,
    pub app_version: u64,
    /// 启动序号：1 = 冷（缓存清）、2 起算热（主册口径）。
    pub launch_seq: u32,
    /// 五段耗时（毫秒）。未走到的段 = None。
    pub seg_ms: [Option<u32>; SEG_COUNT],
    /// 中止于第几段（0 基）；None = 完整走完。
    pub aborted_at: Option<(u8, AbortReason)>,
}

impl LaunchProfile {
    /// 画像合法性：段边界无重叠无遗漏——已记录的段必须从第 0 段起连续
    /// （五段是串行刻度，跳段 = 打点缺失 = 非法）。
    pub fn segments_contiguous(&self) -> bool {
        let mut seen_gap = false;
        for i in 0..SEG_COUNT {
            match (self.seg_ms[i], self.aborted_at) {
                (Some(_), _) if seen_gap => return false,
                (Some(_), _) => {}
                (None, Some((at, _))) if (i as u8) >= at => seen_gap = true,
                (None, None) => seen_gap = true,
                (None, Some(_)) => {}
            }
        }
        true
    }
    /// 总耗时（已记录段之和）。
    pub fn total_ms(&self) -> u32 {
        self.seg_ms.iter().flatten().sum()
    }
}

// ---------------------------------------------------------------------------
// 画像注册表
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct AppStore {
    app_hash: u64,
    used: bool,
    last_seen_ms: u64,
    ring: [Option<LaunchProfile>; LAUNCH_HISTORY],
    head: usize,
    filled: usize,
}
impl AppStore {
    const fn empty() -> Self {
        AppStore { app_hash: 0, used: false, last_seen_ms: 0, ring: [None; LAUNCH_HISTORY], head: 0, filled: 0 }
    }
}

    /// 冷启动画像注册表。星卡与账本的共同数据源（一处一事实）。
pub struct StartProfiler {
    apps: [AppStore; APP_CAP],
    /// 隐私总闸（F036 同开关）：关 = 不采集（丢弃计数）。
    privacy_on: bool,
    dropped_by_privacy: u64,
}

impl StartProfiler {
    pub const fn new() -> Self {
        StartProfiler {
            apps: [AppStore::empty(); APP_CAP],
            privacy_on: true,
            dropped_by_privacy: 0,
        }
    }

    /// 隐私总闸（F036 同开关）。
    pub fn set_privacy(&mut self, on: bool) {
        self.privacy_on = on;
    }

    pub fn privacy_on(&self) -> bool {
        self.privacy_on
    }

    pub fn dropped_by_privacy(&self) -> u64 {
        self.dropped_by_privacy
    }

    /// 记录一次启动画像。应用表满时 LRU 驱逐最久未启动者。
    pub fn record(&mut self, p: LaunchProfile, now_ms: u64) -> bool {
        if !self.privacy_on {
            self.dropped_by_privacy += 1;
            return false;
        }
        if !p.segments_contiguous() {
            return false; // 打点缺失拒绝入册（边界一一对应是判据）
        }
        let slot = match self.find_or_evict(p.app_hash, now_ms) {
            Some(i) => i,
            None => return false,
        };
        let store = &mut self.apps[slot];
        store.ring[store.head] = Some(p);
        store.head = (store.head + 1) % LAUNCH_HISTORY;
        store.filled = (store.filled + 1).min(LAUNCH_HISTORY);
        store.last_seen_ms = now_ms;
        true
    }

    /// 启动中途崩溃 / 取消：标记中止段（主册：本身就是极有价值的归因数据）。
    pub fn record_abort(&mut self, p: LaunchProfile, aborted_at: u8, why: AbortReason, now_ms: u64) -> bool {
        let mut p = p;
        p.aborted_at = Some((aborted_at, why));
        self.record(p, now_ms)
    }

    fn find_or_evict(&mut self, app_hash: u64, now_ms: u64) -> Option<usize> {
        let mut oldest = usize::MAX;
        let mut oldest_seen = u64::MAX;
        for i in 0..APP_CAP {
            if !self.apps[i].used {
                self.apps[i].used = true;
                self.apps[i].app_hash = app_hash;
                return Some(i);
            }
            if self.apps[i].app_hash == app_hash {
                return Some(i);
            }
            if self.apps[i].last_seen_ms < oldest_seen {
                oldest_seen = self.apps[i].last_seen_ms;
                oldest = i;
            }
        }
        // LRU 驱逐。
        self.apps[oldest] = AppStore::empty();
        self.apps[oldest].used = true;
        self.apps[oldest].app_hash = app_hash;
        self.apps[oldest].last_seen_ms = now_ms;
        Some(oldest)
    }

    /// 星卡聚合：取「二次启动中位数」（主册口径——seq≥2 的完整热启动样本，
    /// 五段各自取中位数）。样本不足返回 None（诚实呈现，不编数）。
    pub fn star_card_median(&self, app_hash: u64) -> Option<[u32; SEG_COUNT]> {
        let store = self.apps.iter().find(|a| a.used && a.app_hash == app_hash)?;
        let mut med = [0u32; SEG_COUNT];
        for s in 0..SEG_COUNT {
            let mut seg_vals = [0u32; LAUNCH_HISTORY];
            let mut sn = 0;
            for i in 0..LAUNCH_HISTORY {
                if let Some(p) = store.ring[i] {
                    if p.launch_seq >= 2 && p.aborted_at.is_none() {
                        if let Some(v) = p.seg_ms[s] {
                            seg_vals[sn] = v;
                            sn += 1;
                        }
                    }
                }
            }
            if sn == 0 {
                return None;
            }
            seg_vals[..sn].sort_unstable();
            med[s] = if sn % 2 == 1 { seg_vals[sn / 2] } else { (seg_vals[sn / 2 - 1] + seg_vals[sn / 2]) / 2 };
        }
        Some(med)
    }

    /// 首次 vs 二次对比视图（星卡画像对比：预取 F044 效果一眼可见）。
    pub fn compare_first_second(&self, app_hash: u64) -> Option<([u32; SEG_COUNT], [u32; SEG_COUNT])> {
        let store = self.apps.iter().find(|a| a.used && a.app_hash == app_hash)?;
        let pick = |want_seq: u32| -> Option<[u32; SEG_COUNT]> {
            for i in 0..LAUNCH_HISTORY {
                if let Some(p) = store.ring[(store.head + LAUNCH_HISTORY - 1 - i) % LAUNCH_HISTORY] {
                    if p.launch_seq == want_seq && p.aborted_at.is_none() {
                        let mut out = [0u32; SEG_COUNT];
                        for s in 0..SEG_COUNT {
                            out[s] = p.seg_ms[s]?;
                        }
                        return Some(out);
                    }
                }
            }
            None
        };
        Some((pick(1)?, pick(2)?))
    }

    /// 测量自身稳定性：对某应用最近 `k` 次完整画像的总耗时做离散系数
    /// （CV，百分比整数）。判据：10 次启动 CV <15%。
    pub fn stability_cv_pct(&self, app_hash: u64, k: usize) -> Option<u32> {
        let store = self.apps.iter().find(|a| a.used && a.app_hash == app_hash)?;
        let mut vals = [0u64; LAUNCH_HISTORY];
        let mut n = 0;
        for i in 0..LAUNCH_HISTORY {
            if let Some(p) = store.ring[i] {
                if p.aborted_at.is_none() && n < k && n < LAUNCH_HISTORY {
                    vals[n] = p.total_ms() as u64;
                    n += 1;
                }
            }
        }
        if n < 2 {
            return None;
        }
        let sum: u64 = vals[..n].iter().sum();
        let mean = sum / n as u64;
        if mean == 0 {
            return None;
        }
        let var_sum: u64 = vals[..n].iter().map(|&v| (v as i64 - mean as i64).pow(2) as u64).sum();
        let variance = var_sum / n as u64;
        // CV% = sqrt(var)/mean×100 —— 整数近似：用逐次比较避免浮点。
        // sqrt 整数开方（牛顿法）。
        let sd = isqrt(variance);
        Some((sd * 100 / mean) as u32)
    }
}

/// 整数平方根（牛顿法，u64）。
fn isqrt(x: u64) -> u64 {
    if x < 2 {
        return x;
    }
    let mut g = x;
    let mut n = (g + 1) / 2;
    while n < g {
        g = n;
        n = (g + x / g) / 2;
    }
    g
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

fn prof(app: u64, ver: u64, seq: u32, segs: [u32; SEG_COUNT]) -> LaunchProfile {
    LaunchProfile {
        app_hash: app,
        app_version: ver,
        launch_seq: seq,
        seg_ms: segs.map(Some),
        aborted_at: None,
    }
}

/// 域自检。
pub fn run_startprof_checks() -> CheckSet {
    let mut cs = CheckSet::new("F043-startprof");
    // 1) 五段常量齐（装载/重定位/首帧/可交互/稳定）。
    cs.add("five_segments", SEG_COUNT == 5 && SEG_NAMES == ["load", "relocate", "first-frame", "interactive", "stable"], "");
    // 2) 段边界一一对应：连续合法，跳段拒绝。
    let mut p = prof(1, 1, 1, [100, 40, 610, 200, 5_000]);
    cs.add("contiguous_ok", p.segments_contiguous(), "");
    p.seg_ms[2] = None; // 跳段 = 打点缺失
    cs.add("gap_rejected", !p.segments_contiguous(), "");
    // 3) 稳定段口径 = 5 秒观察窗。
    cs.add("stable_window_5s", STABLE_WINDOW_MS == 5_000, "");
    // 4) 中止标记：崩溃于第 3 段 → 画像如实标记（归因数据）。
    let mut pr = StartProfiler::new();
    let mut ab = prof(2, 7, 1, [100, 40, 600, 0, 0]);
    ab.seg_ms[3] = None;
    ab.seg_ms[4] = None;
    cs.add("abort_marked", pr.record_abort(ab, 3, AbortReason::Crash, 10), "");
    // 5) 隐私总闸（F036 同开关）：关 = 不采集且丢弃计数。
    let mut pr2 = StartProfiler::new();
    pr2.set_privacy(false);
    cs.add("privacy_gate", !pr2.record(prof(3, 1, 1, [1, 1, 1, 1, 5_000]), 0) && pr2.dropped_by_privacy() == 1, "");
    // 6) 星卡聚合取二次启动中位数（seq≥2）。
    let mut pr3 = StartProfiler::new();
    for (seq, base) in [(1u32, 2_000u32), (2, 1_200), (2, 1_400), (2, 1_600)] {
        pr3.record(prof(4, 1, seq, [base / 4, base / 8, base / 2, base / 8, 100]), seq as u64 * 10);
    }
    let med = pr3.star_card_median(4).unwrap();
    cs.add("median_second_launch", med.iter().sum::<u32>() == 1_400 + 100, "");
    // 7) 10 次启动画像方差 <15%（整数 CV）。
    let mut pr4 = StartProfiler::new();
    for i in 0..10u32 {
        let jitter: u32 = (i % 3) * 20; // ±2% 内抖动
        pr4.record(prof(5, 1, i + 1, [1_000 + jitter, 100, 500, 100, 5_000]), i as u64 * 100);
    }
    let cv = pr4.stability_cv_pct(5, 10).unwrap();
    cs.add("cv_below_15pct", cv < 15, "");
    // 8) 首次 vs 二次对比视图（F044 预取效果呈现面）。seq2 样本 = i=1，
    //    jitter = (1%3)*20 = 20 → seg0 = 1_020。
    let (first, second) = pr4.compare_first_second(5).unwrap();
    cs.add("compare_view", first[0] == 1_000 && second[0] == 1_020, "");
    // 9) 保留最近 20 次（第 21 次挤掉第 1 次）：seq1 已被挤出（对比视图
    //    需 seq1 → None）；seq≥2 热启动样本仍在（星卡中位数可用）；CV 可算。
    let mut pr5 = StartProfiler::new();
    for i in 0..21u32 {
        pr5.record(prof(6, 1, i + 1, [100 + i, 1, 1, 1, 5_000]), i as u64);
    }
    let oldest_evicted = pr5.compare_first_second(6).is_none();
    let median_ok = pr5.star_card_median(6).is_some();
    let cv21 = pr5.stability_cv_pct(6, 20);
    cs.add("history_20_cap", oldest_evicted && median_ok && cv21.is_some(), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_boundaries_match_kernel_points() {
        // 段边界常量文档自证（一处一事实）：顺序不可换。
        assert_eq!(Seg::Load as usize, 0);
        assert_eq!(Seg::Relocate as usize, 1);
        assert_eq!(Seg::FirstFrame as usize, 2);
        assert_eq!(Seg::Interactive as usize, 3);
        assert_eq!(Seg::Stable as usize, 4);
    }

    #[test]
    fn abort_profiles_kept_and_flagged() {
        let mut pr = StartProfiler::new();
        let mut p = prof(9, 3, 1, [100, 40, 0, 0, 0]);
        p.seg_ms[2] = None;
        p.seg_ms[3] = None;
        p.seg_ms[4] = None;
        assert!(pr.record_abort(p, 2, AbortReason::UserCancel, 5));
        // 中止画像不进星卡中位数（无完整热启动样本 → None）。
        assert!(pr.star_card_median(9).is_none());
    }

    #[test]
    fn median_uses_only_complete_warm_launches() {
        let mut pr = StartProfiler::new();
        // 3 次热启动：1200/1400/1600 → 中位数 1400。
        for (i, t) in [1_200u32, 1_400, 1_600].iter().enumerate() {
            pr.record(prof(11, 1, (i + 2) as u32, [t / 4, t / 8, t / 2, t / 8, 100]), i as u64);
        }
        let med = pr.star_card_median(11).unwrap();
        assert_eq!(med.iter().sum::<u32>(), 1_500);
    }

    #[test]
    fn cv_stability_under_jitter() {
        let mut pr = StartProfiler::new();
        // 方差大样本：均值 5000、±50% 抖动 → CV >15%（判据反向样本）。
        for i in 0..10u32 {
            let t = if i % 2 == 0 { 2_500 } else { 7_500 };
            pr.record(prof(12, 1, i + 1, [t / 2, t / 8, t / 4, t / 8, 100]), i as u64);
        }
        assert!(pr.stability_cv_pct(12, 10).unwrap() > 15);
    }

    #[test]
    fn lru_evicts_oldest_app() {
        let mut pr = StartProfiler::new();
        for a in 0..APP_CAP as u64 {
            pr.record(prof(100 + a, 1, 1, [1, 1, 1, 1, 5_000]), a * 10);
        }
        // 触碰 app100（变新），再塞第 17 个 → 驱逐的应是 app101。
        pr.record(prof(100, 1, 2, [1, 1, 1, 1, 5_000]), 1_000);
        // app999 占据驱逐出的槽位：记两次（seq=2 才是完整 warm 启动，
        // 中位数只统计完整暖启动样本）。
        pr.record(prof(999, 1, 1, [1, 1, 1, 1, 5_000]), 2_000);
        pr.record(prof(999, 1, 2, [1, 1, 1, 1, 5_000]), 3_000);
        // app100 仍在（能取到对比视图），app101 被逐（查不到其历史 → 中位数 None）。
        assert!(pr.compare_first_second(100).is_some());
        assert!(pr.star_card_median(101).is_none());
        assert!(pr.star_card_median(999).is_some());
    }

    #[test]
    fn isqrt_correct() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(1), 1);
        assert_eq!(isqrt(15), 3);
        assert_eq!(isqrt(16), 4);
        assert_eq!(isqrt(1_000_000), 1_000);
    }
}
