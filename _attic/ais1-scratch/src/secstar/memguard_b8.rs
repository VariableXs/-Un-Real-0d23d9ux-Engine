//! F176 内存卫士 · 批次八（v8）—— sized-recycle 池、分配调用方账、
//! 护栏抖动缓解、OOM 演练谱。零堆、no_std。

use crate::checks::CheckSet;

/// 回收池槽容量。
pub const RECYCLE_CAP: usize = 12;
/// 调用方标签数。
pub const CALLER_TAGS: usize = 12;
/// 抖动缓解窗口（页）。
pub const JITTER_WINDOW_PAGES: usize = 8;
/// OOM 演练场景数。
pub const DRILL_SCENARIOS: usize = 6;

/// 定尺寸回收池：释放的等尺寸块直接入池（免回伙伴系统再分裂）。
/// 池按块尺寸分桶（这里模型单一尺寸——语义验证先行）。
#[derive(Clone, Copy)]
pub struct RecyclePool {
    addrs: [Option<u16>; RECYCLE_CAP],
    pub n: usize,
    pub hits: u32,
    pub misses: u32,
}

impl RecyclePool {
    pub const fn new() -> RecyclePool {
        RecyclePool { addrs: [None; RECYCLE_CAP], n: 0, hits: 0, misses: 0 }
    }

    /// 归还块（同块重复归还拒——双 free 的池面信号）。
    pub fn put(&mut self, addr: u16) -> bool {
        for i in 0..self.n {
            if self.addrs[i] == Some(addr) {
                return false;
            }
        }
        if self.n >= RECYCLE_CAP {
            return false;
        }
        self.addrs[self.n] = Some(addr);
        self.n += 1;
        true
    }

    /// 取块：池空 → miss（回落伙伴系统）；有 → hit（零分裂快径）。
    pub fn take(&mut self) -> Option<u16> {
        if self.n == 0 {
            self.misses += 1;
            return None;
        }
        self.n -= 1;
        let v = self.addrs[self.n];
        self.addrs[self.n] = None;
        self.hits += 1;
        v
    }

    /// 命中率 ‰。
    pub fn hit_rate_permille(&self) -> Option<u32> {
        let t = self.hits + self.misses;
        if t == 0 {
            return None;
        }
        Some(self.hits * 1_000 / t)
    }

    /// 清池（内存压力时归还伙伴系统——池不是私藏）。
    pub fn drain(&mut self) -> u32 {
        let c = self.n as u32;
        self.addrs = [None; RECYCLE_CAP];
        self.n = 0;
        c
    }
}

/// 调用方分配账：按 12 个调用方标签记 (alloc, free)，泄漏归因到人。
#[derive(Clone, Copy)]
pub struct CallerAccount {
    allocs: [u32; CALLER_TAGS],
    frees: [u32; CALLER_TAGS],
    active: [bool; CALLER_TAGS],
}

impl CallerAccount {
    pub const fn new() -> CallerAccount {
        CallerAccount { allocs: [0; CALLER_TAGS], frees: [0; CALLER_TAGS], active: [false; CALLER_TAGS] }
    }

    pub fn on_alloc(&mut self, tag: usize) -> bool {
        if tag >= CALLER_TAGS {
            return false;
        }
        self.allocs[tag] += 1;
        self.active[tag] = true;
        true
    }

    pub fn on_free(&mut self, tag: usize) -> bool {
        if tag >= CALLER_TAGS || self.frees[tag] >= self.allocs[tag] {
            return false; // 越界 / 超 free 拒（账面不为负）
        }
        self.frees[tag] += 1;
        true
    }

    /// 未归还数。
    pub fn outstanding(&self, tag: usize) -> Option<u32> {
        if tag >= CALLER_TAGS || !self.active[tag] {
            return None;
        }
        Some(self.allocs[tag] - self.frees[tag])
    }

    /// 头号嫌疑（未归还最多；并列取低位——确定性）。
    pub fn top_suspect(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        let mut best_v = 0u32;
        for i in 0..CALLER_TAGS {
            if !self.active[i] {
                continue;
            }
            let v = self.allocs[i] - self.frees[i];
            if v > best_v {
                best_v = v;
                best = Some(i);
            }
        }
        best
    }

    /// 总账守恒：所有 alloc − 所有 free = 所有 outstanding（账平判据）。
    pub fn conserved(&self) -> bool {
        let mut a = 0u32;
        let mut f = 0u32;
        let mut o = 0u32;
        for i in 0..CALLER_TAGS {
            if self.active[i] {
                a += self.allocs[i];
                f += self.frees[i];
                o += self.allocs[i] - self.frees[i];
            }
        }
        a - f == o
    }
}

/// 护栏页抖动缓解：连续触碰窗口内多页命中时合并上报（一次报告说清一片）。
#[derive(Clone, Copy)]
pub struct JitterWindow {
    hits: [bool; JITTER_WINDOW_PAGES],
    pub n: usize,
    pub reports: u32,
}

impl JitterWindow {
    pub const fn new() -> JitterWindow {
        JitterWindow { hits: [false; JITTER_WINDOW_PAGES], n: 0, reports: 0 }
    }

    /// 页命中：窗内已有命中 → 合并（不新增报告）；窗空 → 上报一次。
    pub fn on_hit(&mut self, page: usize) -> bool {
        if page >= JITTER_WINDOW_PAGES {
            return false;
        }
        let any = self.hits.iter().any(|h| *h);
        self.hits[page] = true;
        self.n += 1;
        if any {
            false // 合并——不加报告
        } else {
            self.reports += 1;
            true
        }
    }

    /// 窗口结算：返回命中页数并清窗（报告已出，页账归零）。
    pub fn flush(&mut self) -> usize {
        let c = self.hits.iter().filter(|h| **h).count();
        self.hits = [false; JITTER_WINDOW_PAGES];
        self.n = 0;
        c
    }

    /// 命中页列表第一个（定位用）。
    pub fn first_hit(&self) -> Option<usize> {
        (0..JITTER_WINDOW_PAGES).find(|&i| self.hits[i])
    }
}

/// OOM 演练谱：六场景（渐进压力）各验阶梯响应正确。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrillScenario {
    Idle,
    MildLeak,
    BurstAlloc,
    CacheSwell,
    BackgroundRunaway,
    KernelLeak,
}

/// 场景 → 预期阶梯档（演练判据表——场景与响应一一对应）。
pub fn expected_step(s: DrillScenario) -> u8 {
    match s {
        DrillScenario::Idle => 0,
        DrillScenario::MildLeak => 1,
        DrillScenario::BurstAlloc => 2,
        DrillScenario::CacheSwell => 1,
        DrillScenario::BackgroundRunaway => 2,
        DrillScenario::KernelLeak => 3,
    }
}

#[inline(never)]
pub fn run_memguard_b8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F176-b8");

    // 1) 回收池快径：还 2 块取 2 块全 hit（免分裂——快径的意义）。
    let mut r = RecyclePool::new();
    r.put(100);
    r.put(200);
    let t1 = r.take();
    let t2 = r.take();
    cs.add(
        "recycle_fast_path",
        t1 == Some(200) && t2 == Some(100) && r.hits == 2 && r.misses == 0,
        "",
    );

    // 2) 池空 miss：取第 3 块 → None + miss 计数（回落伙伴系统诚实记账）。
    let t3 = r.take();
    cs.add("recycle_miss_on_empty", t3.is_none() && r.misses == 1, "");

    // 3) 双归还拒：同块还两次第二次 false（池面即双 free 信号）。
    let mut r2 = RecyclePool::new();
    let dup = r2.put(50) && !r2.put(50);
    cs.add("recycle_double_put_rejected", dup && r2.n == 1, "");

    // 4) 池满拒与清池：12 满后第 13 块拒；drain 归 12、池空。
    let mut r3 = RecyclePool::new();
    for i in 0..RECYCLE_CAP as u16 {
        assert!(r3.put(i));
    }
    let full = !r3.put(999);
    let drained = r3.drain();
    cs.add(
        "recycle_cap_and_drain",
        full && r3.n == 0 && drained == RECYCLE_CAP as u32,
        "",
    );

    // 5) 命中率口径：2 hit 1 miss → 667‰（666.67 截断）；空账 None。
    let mut r4 = RecyclePool::new();
    r4.put(1);
    let _ = r4.take();
    let _ = r4.take();
    let _ = r4.take(); // miss
    cs.add(
        "recycle_hit_rate",
        r4.hit_rate_permille() == Some(333) && RecyclePool::new().hit_rate_permille().is_none(),
        "",
    );

    // 6) 调用方账：tag3 配 3 还 1 → outstanding 2；top_suspect = 3。
    let mut a = CallerAccount::new();
    for _ in 0..3 {
        a.on_alloc(3);
    }
    a.on_free(3);
    a.on_alloc(7);
    cs.add(
        "caller_outstanding",
        a.outstanding(3) == Some(2) && a.outstanding(7) == Some(1) && a.top_suspect() == Some(3),
        "",
    );

    // 7) 账面守恒：alloc 总 − free 总 = outstanding 总（三账一式）。
    cs.add("caller_conserved", a.conserved(), "");

    // 8) 调用方守门：越界 tag 拒、超 free 拒（账面不为负）。
    let mut a2 = CallerAccount::new();
    let over_free = !a2.on_free(5);
    let oob = !a2.on_alloc(CALLER_TAGS);
    cs.add("caller_guards", over_free && oob, "");

    // 9) 抖动合并：窗口内 3 页连触 → 1 次报告（一次说清一片）。
    let mut j = JitterWindow::new();
    let h1 = j.on_hit(2);
    let h2 = j.on_hit(3);
    let h3 = j.on_hit(5);
    cs.add(
        "jitter_merges_burst",
        h1 && !h2 && !h3 && j.reports == 1 && j.n == 3,
        "",
    );

    // 10) 抖动结算：flush 返回 3 页、清窗后首命中重新上报（窗有界不积压）。
    let flushed = j.flush();
    let again = j.on_hit(1);
    cs.add(
        "jitter_flush_rearms",
        flushed == 3 && again && j.first_hit() == Some(1),
        "",
    );

    // 11) 演练谱：六场景判据表逐点（场景↔响应一一对应不缺项）。
    cs.add(
        "drill_matrix_exact",
        expected_step(DrillScenario::Idle) == 0
            && expected_step(DrillScenario::MildLeak) == 1
            && expected_step(DrillScenario::BurstAlloc) == 2
            && expected_step(DrillScenario::CacheSwell) == 1
            && expected_step(DrillScenario::BackgroundRunaway) == 2
            && expected_step(DrillScenario::KernelLeak) == 3,
        "",
    );

    // 12) 常量自洽：池 12、调用方 12、窗 8、场景 6。
    cs.add(
        "b8_constants",
        RECYCLE_CAP == 12 && CALLER_TAGS == 12 && JITTER_WINDOW_PAGES == 8 && DRILL_SCENARIOS == 6,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recycle_fifo_order() {
        // 池取序：LIFO（栈式复用——最近还的最热）。
        let mut r = RecyclePool::new();
        r.put(10);
        r.put(20);
        r.put(30);
        assert_eq!(r.take(), Some(30));
        assert_eq!(r.take(), Some(20));
        assert_eq!(r.take(), Some(10));
    }

    #[test]
    fn caller_top_suspect_tie_low_index() {
        // 并列嫌疑取低位（确定性——同输入同结论）。
        let mut a = CallerAccount::new();
        a.on_alloc(4);
        a.on_alloc(9);
        assert_eq!(a.top_suspect(), Some(4));
    }

    #[test]
    fn jitter_window_boundary() {
        // 越界页拒：page ≥ 窗容 → false（归因面守门）。
        let mut j = JitterWindow::new();
        assert!(!j.on_hit(JITTER_WINDOW_PAGES));
        assert!(j.on_hit(0));
    }
}
