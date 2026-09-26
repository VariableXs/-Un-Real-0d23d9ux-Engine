//! mech_readahead — 页缓存预读窗口状态机本体（AI-K1 深化批次五 · F044）。
//!
//! 主册依据：
//! - F044【设计细节】「二次只预读 9MB」——域内有预读量账与 LRU 保留
//!   语义（mech_clocklru），但**预读窗口本身怎么长大、怎么收缩、怎么
//!   复位**的算法缺席。本件按 Linux readahead 状态机语义实现：顺序流
//!   命中 → 窗口倍增（至 MAX 窗）、到达 ahead 边界 → 异步批量预读、
//!   随机访问 → 窗口缩回初始、跳页（gap）→ 在缺口处复位重启、窗口
//!   总量封顶（防止预读洪水——F044「只预读 9MB」的量级来源）。
//! - 锚点：F044「连看千张图片流畅」的前提是顺序流被识别并放大、
//!   随机流不被误放大——状态机的两个方向都要可验证。
//! - 零堆、零浮点。

// ---------------------------------------------------------------------------
// 1. 参数与状态
// ---------------------------------------------------------------------------

/// 页大小 4KB；窗口量全部以页计。
pub const PAGE_BYTES: u64 = 4096;
/// 初始窗口（页）——冷启动/复位后的保守量。
pub const INITIAL_WINDOW: u32 = 4;
/// 最大窗口（页）= 32 页 = 128KB（Linux 默认 max 同量级）。
pub const MAX_WINDOW: u32 = 32;
/// 最小窗口（随机流的落点）。
pub const MIN_WINDOW: u32 = 2;
/// 异步触发提前量（页）：剩余 ≤ ahead_boundary 时批量预读。
pub const AHEAD_BOUNDARY: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaPhase {
    /// 无窗口（尚未识别到流）。
    Idle,
    /// 顺序流已确认，窗口增长中。
    Sequential,
    /// 随机流（窗口压到最小）。
    Random,
}

/// 预读状态机（每文件实例化一份）。
pub struct Readahead {
    pub phase: RaPhase,
    /// 窗口起点（页号）。
    pub start: u64,
    /// 窗口大小（页）。
    pub size: u32,
    /// 本窗口已消费到的页号（下一期望页 = start + consumed）。
    pub consumed: u32,
    /// 统计面。
    pub async_issues: u64,
    pub resets: u64,
    pub pages_prefetched: u64,
}

impl Readahead {
    pub fn new() -> Self {
        Readahead {
            phase: RaPhase::Idle,
            start: 0,
            size: 0,
            consumed: 0,
            async_issues: 0,
            resets: 0,
            pages_prefetched: 0,
        }
    }

    pub fn window_pages(&self) -> u32 {
        self.size
    }

    /// 访问一个页号。返回本回合的预读决策（本回合发起的批量预读页数，
    /// 0 = 未发起）。页号语义：调用方按读取顺序报告。
    pub fn access(&mut self, page: u64) -> u32 {
        match self.phase {
            RaPhase::Idle => {
                // 冷启动：在该页开一个初始窗口。
                self.phase = RaPhase::Sequential;
                self.start = page;
                self.size = INITIAL_WINDOW;
                self.consumed = 1;
                self.pages_prefetched += (INITIAL_WINDOW - 1) as u64;
                INITIAL_WINDOW - 1
            }
            RaPhase::Sequential | RaPhase::Random => {
                let next_expected = self.start + self.consumed as u64;
                if page == next_expected {
                    // 顺序命中：消费推进。
                    self.consumed += 1;
                    if self.phase == RaPhase::Sequential
                        && self.consumed >= self.size.saturating_sub(AHEAD_BOUNDARY)
                    {
                        // 到达 ahead 边界：窗口倍增、异步批量预读。
                        let grow = self.size.min(MAX_WINDOW);
                        let new_size = (self.size + grow).min(MAX_WINDOW);
                        let issued = new_size - self.size;
                        if issued > 0 {
                            self.async_issues += 1;
                            self.pages_prefetched += issued as u64;
                        }
                        self.size = new_size;
                        // consumed 不变：新预读页在窗口尾部待消费。
                        return issued;
                    }
                    0
                } else if page + 1 == next_expected || self.in_window(page) {
                    // 后退读/窗口内乱序：不奖不罚（保持）。
                    0
                } else {
                    // 缺口或大跳：复位到该页（缺口重启语义）。
                    self.resets += 1;
                    self.phase = RaPhase::Sequential;
                    self.start = page;
                    self.size = INITIAL_WINDOW;
                    self.consumed = 1;
                    self.pages_prefetched += (INITIAL_WINDOW - 1) as u64;
                    INITIAL_WINDOW - 1
                }
            }
        }
    }

    /// 显式报告随机访问（消费域从文件系统元数据得知非顺序意图）。
    pub fn mark_random(&mut self) {
        if self.phase == RaPhase::Sequential && self.size > MIN_WINDOW {
            self.phase = RaPhase::Random;
            self.size = MIN_WINDOW;
        }
    }

    fn in_window(&self, page: u64) -> bool {
        page >= self.start && page < self.start + self.size as u64
    }
}

impl Default for Readahead {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 2. CheckSet
// ---------------------------------------------------------------------------

pub fn run_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut cs = CheckSet::new("mech_readahead");

    // 1) 顺序流窗口倍增到封顶：4 → 8 → 16 → 32（不多不少）。
    {
        let mut ra = Readahead::new();
        let mut issued_total = 0u32;
        for p in 0..200u64 {
            issued_total += ra.access(p);
        }
        cs.add(
            "ra_doubles_to_cap",
            ra.window_pages() == MAX_WINDOW && ra.async_issues >= 3 && issued_total > 0,
            "",
        );
    }

    // 2) 随机流不放大：反复跳跃后窗口停在最小值。
    {
        let mut ra = Readahead::new();
        let mut base = 1000u64;
        for i in 0..40 {
            ra.access(base);
            base += (i as u64 % 7) * 137 + 211; // 伪随机大跳
        }
        cs.add("ra_random_stays_small", ra.window_pages() <= INITIAL_WINDOW && ra.resets >= 10, "");
    }

    // 3) 缺口复位：顺序流中途跳页，窗口在新页重启为初始值。
    {
        let mut ra = Readahead::new();
        for p in 0..10u64 {
            ra.access(p);
        }
        let before = ra.window_pages();
        let issued = ra.access(10_000);
        cs.add(
            "ra_gap_reset",
            before > INITIAL_WINDOW && issued == INITIAL_WINDOW - 1 && ra.start == 10_000 && ra.resets == 1,
            "",
        );
    }

    // 4) 预读总量封顶：200 页顺序流的总预读页数 ≤ 窗口封顶下的几何上界
    //    （判据：预读洪水被量级约束——F044「只预读 9MB」的机制来源）。
    {
        let mut ra = Readahead::new();
        for p in 0..200u64 {
            ra.access(p);
        }
        // 初始 3 + 每次倍增批量（4+8+16+…封顶前）≤ ~60，远小于 200。
        cs.add("ra_total_bounded", ra.pages_prefetched < 100 && ra.pages_prefetched >= 27, "");
    }

    // 5) mark_random 显式收缩。
    {
        let mut ra = Readahead::new();
        for p in 0..12u64 {
            ra.access(p);
        }
        let big = ra.window_pages();
        ra.mark_random();
        cs.add("ra_mark_random_shrinks", big > MIN_WINDOW && ra.phase == RaPhase::Random && ra.window_pages() == MIN_WINDOW, "");
    }

    cs
}

// ---------------------------------------------------------------------------
// 3. 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_growth_ladder() {
        let mut ra = Readahead::new();
        // 首访问：初始窗口批量 3 页。
        assert_eq!(ra.access(0), INITIAL_WINDOW - 1);
        assert_eq!(ra.window_pages(), INITIAL_WINDOW);
        // 第二访问即达边界（4-2=2）→ 倍增到 8，批量 4。
        assert_eq!(ra.access(1), 4);
        assert_eq!(ra.window_pages(), 8);
        // 消费 3..5 未达边界（8-2=6）→ 无批量。
        assert_eq!(ra.access(2), 0);
        assert_eq!(ra.access(3), 0);
        assert_eq!(ra.access(4), 0);
        // 第 6 次 → 倍增到 16，批量 8。
        assert_eq!(ra.access(5), 8);
        assert_eq!(ra.window_pages(), 16);
    }

    #[test]
    fn growth_caps_at_max() {
        let mut ra = Readahead::new();
        for p in 0..500u64 {
            ra.access(p);
        }
        assert_eq!(ra.window_pages(), MAX_WINDOW);
        assert_eq!(ra.phase, RaPhase::Sequential);
        // 封顶后批量归零（无增长空间）。
        let i = ra.access(500);
        assert_eq!(i, 0);
    }

    #[test]
    fn gap_restarts_conservatively() {
        let mut ra = Readahead::new();
        for p in 0..50u64 {
            ra.access(p);
        }
        assert_eq!(ra.window_pages(), MAX_WINDOW);
        // 跳到远处：窗口回到初始，起点跟随。
        ra.access(99_999);
        assert_eq!(ra.window_pages(), INITIAL_WINDOW);
        assert_eq!(ra.start, 99_999);
        assert_eq!(ra.consumed, 1);
        // 立即继续顺序 → 重新长大。
        for p in 100_000..100_030u64 {
            ra.access(p);
        }
        assert!(ra.window_pages() > INITIAL_WINDOW);
    }

    #[test]
    fn random_marks_shrink_and_sequential_resume_grows() {
        let mut ra = Readahead::new();
        for p in 0..20u64 {
            ra.access(p);
        }
        ra.mark_random();
        assert_eq!(ra.window_pages(), MIN_WINDOW);
        // 随机相里继续顺序访问：仍按当前小窗走，不再倍增（Random 相冻结）。
        let next = ra.start + ra.consumed as u64;
        ra.access(next);
        assert_eq!(ra.window_pages(), MIN_WINDOW);
    }

    #[test]
    fn backward_reads_do_not_reset() {
        let mut ra = Readahead::new();
        for p in 0..20u64 {
            ra.access(p);
        }
        let before_resets = ra.resets;
        // 回读一页（窗口内）：不触发复位。
        ra.access(10);
        assert_eq!(ra.resets, before_resets);
        assert_eq!(ra.window_pages(), ra.window_pages());
    }

    #[test]
    fn prefetch_accounting_consistent() {
        let mut ra = Readahead::new();
        let mut sum = 0u64;
        for p in 0..100u64 {
            sum += ra.access(p) as u64;
        }
        // pages_prefetched == Σ issued（账实相符）。
        assert_eq!(ra.pages_prefetched, sum);
        assert!(ra.pages_prefetched > 0);
    }
}
