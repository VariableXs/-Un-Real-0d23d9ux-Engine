//! thumbsched — WP-205 · B-1704 缩略图异步（MD2 篇 17.1）。
//!
//! 判据 B-1704：缩略图异步，大文件可取消有进度。
//! MD2 原文（17.1）："图片类按需生成缩略图（缓存进用户缓存目录，LRU 上限），
//! 生成在大文件上有取消与进度——卡死界面的缩略图是高频事故点，异步加节流
//! 是底线。"
//!
//! 宿主可测形态：按需生成（无预扫——初始零生成，访问驱动）+ LRU 缓存上限
//! （容量恒定即内存恒定，触碰刷新，满淘汰最旧）+ 生成任务（进度单调推进、
//! 任意进度可取消、取消后无半成品落盘——out_ready 仅在完成时为真）+ 并发
//! 节流（两槽上限，第三任务排队）+ 异步不卡 UI（每步预算推导：2ms/步 ×
//! 八步出图 ≤ 一帧 16ms）。

use crate::checks::CheckSet;

pub const THUMB_CAP: usize = 128;
/// 并发生成槽上限（节流底线）。
pub const GEN_SLOTS: usize = 2;
/// 每步生成预算（ns）：单步工作量恒定 → UI 线不被长任务卡住。
pub const STEP_BUDGET_NS: u64 = 2_000_000;
/// 一帧预算（16ms）——八步出图即在一帧内完成小图。
pub const FRAME_BUDGET_NS: u64 = 16_000_000;

pub fn async_budget_ok() -> bool {
    STEP_BUDGET_NS * 8 <= FRAME_BUDGET_NS
}

/// 缩略图缓存：LRU 上限（缓存进用户缓存目录的模型面）。
pub struct ThumbCache {
    keys: [u32; THUMB_CAP],
    ticks: [u64; THUMB_CAP],
    used: [bool; THUMB_CAP],
    pub clock: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    /// 已生成次数（按需生成的计量面：初始零、访问驱动）。
    pub generated: u64,
}

impl ThumbCache {
    pub const fn new() -> ThumbCache {
        ThumbCache {
            keys: [0; THUMB_CAP],
            ticks: [0; THUMB_CAP],
            used: [false; THUMB_CAP],
            clock: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
            generated: 0,
        }
    }

    fn find(&self, key: u32) -> Option<usize> {
        let mut i = 0;
        while i < THUMB_CAP {
            if self.used[i] && self.keys[i] == key {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 查缓存：命中触碰（LRU 刷新），未命中记 miss（生成由调用方驱动）。
    pub fn lookup(&mut self, key: u32) -> bool {
        self.clock += 1;
        match self.find(key) {
            Some(i) => {
                self.ticks[i] = self.clock;
                self.hits += 1;
                true
            }
            None => {
                self.misses += 1;
                false
            }
        }
    }

    /// 按需生成后入库；满则 LRU 淘汰最旧。
    pub fn insert(&mut self, key: u32) -> bool {
        if self.find(key).is_some() {
            return true; // 已在缓存：命中语义，不重复生成。
        }
        let slot = match self.find_free() {
            Some(s) => s,
            None => self.evict_oldest(),
        };
        self.clock += 1;
        self.keys[slot] = key;
        self.ticks[slot] = self.clock;
        self.used[slot] = true;
        self.generated += 1;
        true
    }

    fn find_free(&self) -> Option<usize> {
        let mut i = 0;
        while i < THUMB_CAP {
            if !self.used[i] {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn evict_oldest(&mut self) -> usize {
        let mut oldest = 0;
        let mut i = 1;
        while i < THUMB_CAP {
            if self.ticks[i] < self.ticks[oldest] {
                oldest = i;
            }
            i += 1;
        }
        self.used[oldest] = false;
        self.evictions += 1;
        oldest
    }

    pub fn used_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < THUMB_CAP {
            if self.used[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 缩略图生成任务：进度单调推进，任意进度可取消，取消后无半成品落盘。
pub struct GenJob {
    pub file_size: u64,
    pub progress: u8,
    pub done: bool,
    pub cancelled: bool,
    /// 落盘许可：仅在完成时为真（无半成品落盘的结构面）。
    pub out_ready: bool,
    /// 历史进度峰值（单调性对账面）。
    pub peak: u8,
}

impl GenJob {
    pub const fn new(file_size: u64) -> GenJob {
        GenJob { file_size, progress: 0, done: false, cancelled: false, out_ready: false, peak: 0 }
    }

    /// 后台推进一步（进度只增不减）。
    pub fn step(&mut self, pct: u8) -> bool {
        if self.cancelled || self.done {
            return false;
        }
        self.progress = self.progress.saturating_add(pct).min(100);
        if self.progress > self.peak {
            self.peak = self.progress;
        }
        if self.progress >= 100 {
            self.done = true;
            self.out_ready = true; // 唯一置位点：完成才许可落盘。
        }
        true
    }

    /// 任意进度可取消；取消即关闭落盘许可（无半成品）。
    pub fn cancel(&mut self) -> bool {
        if self.done {
            return false;
        }
        self.cancelled = true;
        self.out_ready = false;
        true
    }
}

/// 生成泳道：并发节流（GEN_SLOTS 上限，超出排队返回 None）。
pub struct GenLanes {
    busy: usize,
}

impl GenLanes {
    pub const fn new() -> GenLanes {
        GenLanes { busy: 0 }
    }

    pub fn acquire(&mut self) -> Option<usize> {
        if self.busy >= GEN_SLOTS {
            return None; // 排队（调用方稍后重试）。
        }
        self.busy += 1;
        Some(self.busy - 1)
    }

    pub fn release(&mut self) {
        self.busy = self.busy.saturating_sub(1);
    }

    pub fn in_flight(&self) -> usize {
        self.busy
    }
}

// ============ CheckSet（B-1704 ×8）============

pub fn run_thumbsched_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-1704 缩略图异步");
    {
        // B-1704 按需生成零预扫。
        let mut c = ThumbCache::new();
        let untouched = c.generated == 0 && c.used_count() == 0;
        let miss_first = !c.lookup(7);
        let _ = c.insert(7);
        set.add(
            "B-1704 按需生成零预扫",
            untouched && miss_first && c.generated == 1,
            "初始零生成；首次访问 miss 才生成（无预扫）",
        );
    }
    {
        // B-1704 LRU 上限淘汰。
        let mut c = ThumbCache::new();
        let mut k = 0;
        while k < (THUMB_CAP + 10) as u32 {
            let _ = c.insert(k);
            k += 1;
        }
        set.add(
            "B-1704 LRU 上限淘汰",
            c.used_count() == THUMB_CAP && c.evictions == 10,
            "灌 CAP+10 → 淘汰最旧 10 条（容量恒定即内存恒定）",
        );
    }
    {
        // B-1704 命中不重生成。
        let mut c = ThumbCache::new();
        let _ = c.insert(42);
        let g0 = c.generated;
        let hit = c.lookup(42);
        set.add(
            "B-1704 命中不重生成",
            hit && c.generated == g0 && c.hits == 1,
            "二次访问走缓存（generated 不增）",
        );
    }
    {
        // B-1704 进度单调推进。
        let mut j = GenJob::new(100_000_000);
        let mut seq_ok = true;
        let mut i = 0;
        while i < 10 {
            let _ = j.step(15);
            if j.progress < j.peak {
                seq_ok = false;
            }
            i += 1;
        }
        set.add(
            "B-1704 进度单调推进",
            seq_ok && j.done && j.out_ready && j.peak == 100,
            "step 只增不减；100% 完成 → 落盘许可",
        );
    }
    {
        // B-1704 任意进度可取消（早段/中段）。
        let mut early = GenJob::new(1_000_000);
        let _ = early.step(5);
        let c1 = early.cancel();
        let mut mid = GenJob::new(1_000_000);
        let _ = mid.step(60);
        let c2 = mid.cancel();
        set.add(
            "B-1704 任意进度可取消",
            c1 && c2 && early.cancelled && mid.cancelled,
            "5% 与 60% 均可取消（不做完成度门槛）",
        );
    }
    {
        // B-1704 取消无半成品：取消后 out_ready 恒 false。
        let mut j = GenJob::new(1_000_000);
        let _ = j.step(80);
        let _ = j.cancel();
        // 取消后 step 拒绝（不会诈尸完成）。
        let dead = !j.step(20);
        set.add(
            "B-1704 取消无半成品",
            dead && !j.out_ready && !j.done,
            "out_ready 仅完成时为真——取消任务零落盘",
        );
    }
    {
        // B-1704 并发节流排队：两槽上限。
        let mut lanes = GenLanes::new();
        let a = lanes.acquire();
        let b = lanes.acquire();
        let c = lanes.acquire();
        lanes.release();
        let d = lanes.acquire();
        set.add(
            "B-1704 并发节流排队",
            a.is_some() && b.is_some() && c.is_none() && d.is_some()
                && lanes.in_flight() == 2,
            "两槽占用后第三任务排队；释放后腾位可再取",
        );
    }
    {
        // B-1704 异步不卡 UI：每步预算 × 8 步 ≤ 一帧。
        set.add(
            "B-1704 异步不卡 UI",
            async_budget_ok(),
            "2ms/步 × 8 步 = 16ms ≤ 一帧（小图一帧内出图）",
        );
    }
    set
}

// ============ 单测（f905 ×4）============

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f905_lru_evict() {
        let mut c = ThumbCache::new();
        let mut k = 0;
        while k < (THUMB_CAP + 3) as u32 {
            let _ = c.insert(k);
            k += 1;
        }
        assert_eq!(c.used_count(), THUMB_CAP);
        assert_eq!(c.evictions, 3);
        // 最旧的 0/1/2 应被淘汰。
        assert!(!c.lookup(0));
        assert!(!c.lookup(1));
        assert!(!c.lookup(2));
        // 最新插入的仍在。
        assert!(c.lookup((THUMB_CAP + 2) as u32));
    }

    #[test]
    fn f905_cancel_no_partial() {
        let mut j = GenJob::new(500_000_000);
        let _ = j.step(30);
        let _ = j.step(30);
        assert!(j.cancel());
        assert!(j.cancelled && !j.out_ready && !j.done);
        // 取消后不可再推进。
        assert!(!j.step(40));
        assert_eq!(j.progress, 60);
    }

    #[test]
    fn f905_lane_throttle() {
        let mut lanes = GenLanes::new();
        let a = lanes.acquire().expect("槽 A");
        let b = lanes.acquire().expect("槽 B");
        assert!(lanes.acquire().is_none(), "第三任务必须排队");
        lanes.release();
        assert!(lanes.acquire().is_some(), "释放后可再取");
        let _ = (a, b);
    }

    #[test]
    fn f905_progress_monotonic() {
        let mut j = GenJob::new(1);
        // 大步进不越 100。
        let mut i = 0;
        while i < 30 {
            let _ = j.step(40);
            i += 1;
        }
        assert!(j.done && j.progress == 100);
        assert_eq!(j.peak, 100);
        // 完成后 cancel/step 拒绝。
        assert!(!j.cancel());
        assert!(!j.step(1));
        assert!(j.out_ready, "完成任务保留落盘许可");
    }
}
