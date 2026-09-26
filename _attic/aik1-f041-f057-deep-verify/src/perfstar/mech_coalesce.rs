//! mech_coalesce — 自适应中断合并本体（AI-K1 深化批次五 · F050）。
//!
//! 主册依据：
//! - F050【设计细节】「每秒几百个滚轮事件——跟手不跳」——域内有事件
//!   账（输入指纹、时间轮管定时），但**中断合并窗口本身怎么伸缩**的
//!   算法缺席。本件按 NIC interrupt coalescing / 输入批处理语义实现：
//!   事件到达 → 攒入当前窗口；窗口到期或延迟硬上限先到 → 一次性上报。
//!   窗口自适应：高事件率拉长（省中断）、事件率降或出现"单事件等满窗"
//!   时缩短（保延迟）。两条硬判据：① 最坏事件延迟 ≤ MAX_LATENCY_US
//!   （跟手的硬底线）；② 高速率下中断数显著少于事件数（合并的意义）。
//! - 锚点：F050「跟手不跳」= 延迟上限判据；「省电」= 中断数下降判据。
//!   两面分开验，谁也不能吃掉谁。
//! - 零堆、零浮点（事件率用 EWMA 定点）。

// ---------------------------------------------------------------------------
// 1. 参数
// ---------------------------------------------------------------------------

/// 延迟硬上限（μs）——任何事件的等待不得超过此值（跟手底线）。
pub const MAX_LATENCY_US: u64 = 2000;
/// 窗口下限/上限（μs）。
pub const MIN_WINDOW_US: u64 = 100;
pub const MAX_WINDOW_US: u64 = 1000;
/// 窗口伸缩步长（μs）。
pub const WINDOW_STEP_US: u64 = 100;
/// 窗口内最多攒的事件数（批上限——防窗口尾风暴）。
pub const MAX_BATCH: usize = 32;
/// 事件间隔 EWMA 平滑系数（×256 定点：8/256）。
const ALPHA: u64 = 8;

/// 一个到达的事件（种类 + 到达时刻 μs）。
#[derive(Clone, Copy, Debug)]
pub struct Ev {
    pub kind: u8,
    pub at_us: u64,
}

/// 已交付的一批（中断）。
#[derive(Clone, Copy, Debug)]
pub struct Batch {
    pub count: usize,
    /// 批内最老事件的年龄（μs）——延迟判据的测量面。
    pub max_age_us: u64,
    pub kinds: [u8; MAX_BATCH],
}

// ---------------------------------------------------------------------------
// 2. 合并器
// ---------------------------------------------------------------------------

pub struct Coalescer {
    window_us: u64,
    window_opened_us: u64,
    /// 攒批区。
    pending: [Ev; MAX_BATCH],
    pending_n: usize,
    /// 事件间隔 EWMA（μs，×256 定点；0 = 尚无样本）。
    gap_ewma_x256: u64,
    last_at_us: u64,
    /// 统计面。
    pub events_in: u64,
    pub interrupts_out: u64,
    pub max_wait_us: u64,
    pub window_grew: u64,
    pub window_shrank: u64,
}

impl Coalescer {
    pub fn new() -> Self {
        Coalescer {
            window_us: MIN_WINDOW_US,
            window_opened_us: 0,
            pending: [Ev { kind: 0, at_us: 0 }; MAX_BATCH],
            pending_n: 0,
            gap_ewma_x256: 0,
            last_at_us: 0,
            events_in: 0,
            interrupts_out: 0,
            max_wait_us: 0,
            window_grew: 0,
            window_shrank: 0,
        }
    }

    pub fn window_us(&self) -> u64 {
        self.window_us
    }

    pub fn pending_n(&self) -> usize {
        self.pending_n
    }

    /// 事件到达。返回 Some(Batch) 表示本次到达触发了上报（窗口满/批满/
    /// 延迟上限）。
    pub fn push(&mut self, e: Ev) -> Option<Batch> {
        // 间隔 EWMA（首个样本直接锚定）。
        if self.events_in > 0 {
            let gap = e.at_us.saturating_sub(self.last_at_us).min(1 << 20);
            self.gap_ewma_x256 = if self.gap_ewma_x256 == 0 {
                gap * 256
            } else {
                (self.gap_ewma_x256 * (256 - ALPHA) + gap * ALPHA) / 256
            };
        }
        self.last_at_us = e.at_us;
        self.events_in += 1;

        // 批满：立即上报（不与容量对抗）。
        if self.pending_n >= MAX_BATCH {
            let b = self.flush(e.at_us);
            self.adapt();
            // 本事件入新批。
            self.pending[0] = e;
            self.pending_n = 1;
            self.window_opened_us = e.at_us;
            return Some(b);
        }
        self.pending[self.pending_n] = e;
        self.pending_n += 1;

        // 上报条件：攒龄达到延迟硬上限（最老事件已等太久）。
        let oldest = self.pending[0].at_us;
        let wait = e.at_us.saturating_sub(oldest);
        if wait >= MAX_LATENCY_US {
            let b = self.flush(e.at_us);
            self.adapt();
            return Some(b);
        }
        // 窗口到期（自开窗起经过 window_us 且批内有货）。
        if self.pending_n > 0 && e.at_us.saturating_sub(self.window_opened_us) >= self.window_us {
            let b = self.flush(e.at_us);
            self.adapt();
            return Some(b);
        }
        None
    }

    /// 定时器驱动：时刻 `now_us` 的窗口检查（空闲路径——最后一批等窗口）。
    pub fn tick(&mut self, now_us: u64) -> Option<Batch> {
        if self.pending_n == 0 {
            return None;
        }
        if now_us.saturating_sub(self.window_opened_us) >= self.window_us
            || now_us.saturating_sub(self.pending[0].at_us) >= MAX_LATENCY_US
        {
            let b = self.flush(now_us);
            self.adapt();
            return Some(b);
        }
        None
    }

    fn flush(&mut self, now_us: u64) -> Batch {
        let mut kinds = [0u8; MAX_BATCH];
        for k in 0..self.pending_n {
            kinds[k] = self.pending[k].kind;
        }
        let max_age = now_us.saturating_sub(self.pending[0].at_us);
        if max_age > self.max_wait_us {
            self.max_wait_us = max_age;
        }
        let b = Batch { count: self.pending_n, max_age_us: max_age, kinds };
        self.interrupts_out += 1;
        self.pending_n = 0;
        // 新窗口从上报时刻起算（否则旧起点会让后续每事件都误判到期）。
        self.window_opened_us = now_us;
        b
    }

    /// 自适应：间隔 EWMA 小（事件密集）→ 拉长窗口；间隔大（稀疏）→ 缩短。
    fn adapt(&mut self) {
        if self.gap_ewma_x256 == 0 {
            return;
        }
        let gap_us = self.gap_ewma_x256 / 256;
        if gap_us * 4 < self.window_us && self.window_us < MAX_WINDOW_US {
            // 平均间隔远小于窗口 → 有合并余量。
            self.window_us = (self.window_us + WINDOW_STEP_US).min(MAX_WINDOW_US);
            self.window_grew += 1;
        } else if gap_us > self.window_us && self.window_us > MIN_WINDOW_US {
            // 事件比窗口还稀 → 纯延迟无收益。
            self.window_us = self.window_us.saturating_sub(WINDOW_STEP_US).max(MIN_WINDOW_US);
            self.window_shrank += 1;
        }
    }

    /// 最坏等待（审计面）。
    pub fn worst_wait(&self) -> u64 {
        self.max_wait_us
    }
}

impl Default for Coalescer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 3. CheckSet
// ---------------------------------------------------------------------------

pub fn run_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut cs = CheckSet::new("mech_coalesce");

    // 1) 高速率合并：每 10μs 一事件 × 1000 → 中断数 ≤ 100（10:1 起步）。
    {
        let mut c = Coalescer::new();
        let mut t = 0u64;
        let mut batches = 0u64;
        for _ in 0..1000 {
            match c.push(Ev { kind: 1, at_us: t }) {
                Some(_) => batches += 1,
                None => {}
            }
            t += 10;
        }
        if let Some(_) = c.tick(t) {
            batches += 1;
        }
        cs.add("co_high_rate_saves", batches * 10 <= 1000 && c.interrupts_out == batches, "");
    }

    // 2) 延迟硬上限：任何批的最老事件年龄 ≤ MAX_LATENCY_US（+ 事件步进）。
    {
        let mut c = Coalescer::new();
        let mut t = 0u64;
        let mut worst = 0u64;
        for _i in 0..600u64 {
            // 每 200μs 一事件（稀于窗口，但不至于逐事件上报——窗口 100 起）。
            if let Some(b) = c.push(Ev { kind: 2, at_us: t }) {
                worst = worst.max(b.max_age_us);
            }
            t += 200;
        }
        if let Some(b) = c.tick(t) {
            worst = worst.max(b.max_age_us);
        }
        cs.add("co_latency_cap", worst <= MAX_LATENCY_US && c.worst_wait() <= MAX_LATENCY_US, "");
    }

    // 3) 自适应方向：密集流后窗口爬到 900μs 档、稀疏流后缩回下限档。
    {
        let mut c = Coalescer::new();
        let mut t = 0u64;
        for _ in 0..800 {
            let _ = c.push(Ev { kind: 3, at_us: t });
            t += 5; // 极密集
        }
        let dense_w = c.window_us();
        // 稀疏流：每 50ms 一事件（每次 push 都触发 flush+adapt）。
        for _ in 0..200 {
            t += 50_000;
            let _ = c.push(Ev { kind: 3, at_us: t });
        }
        cs.add(
            "co_adaptive_direction",
            dense_w >= MAX_WINDOW_US - WINDOW_STEP_US
                && c.window_us() == MIN_WINDOW_US
                && c.window_grew > 0
                && c.window_shrank > 0,
            "",
        );
    }

    // 4) 批容量硬上限：风暴中批数 ≤ MAX_BATCH（超出即报，不无限攒）。
    {
        let mut c = Coalescer::new();
        let mut t = 0u64;
        let mut max_batch_seen = 0usize;
        for _ in 0..200 {
            if let Some(b) = c.push(Ev { kind: 4, at_us: t }) {
                max_batch_seen = max_batch_seen.max(b.count);
            }
            t += 1;
        }
        cs.add("co_batch_cap", max_batch_seen <= MAX_BATCH && max_batch_seen >= 16, "");
    }

    // 5) 账实相符：events_in == Σ batch.count + pending。
    {
        let mut c = Coalescer::new();
        let mut t = 0u64;
        let mut delivered = 0u64;
        for i in 0..333 {
            if let Some(b) = c.push(Ev { kind: 5, at_us: t }) {
                delivered += b.count as u64;
            }
            t += (i % 5) as u64 * 7 + 3;
        }
        while let Some(b) = c.tick(t) {
            delivered += b.count as u64;
            t += 10_000;
        }
        cs.add(
            "co_accounting",
            delivered + c.pending_n() as u64 == c.events_in && c.events_in == 333,
            "",
        );
    }

    cs
}

// ---------------------------------------------------------------------------
// 4. 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_event_waits_for_window_not_forever() {
        let mut c = Coalescer::new();
        // 单事件：窗口 100μs 内不报。
        assert!(c.push(Ev { kind: 1, at_us: 0 }).is_none());
        assert_eq!(c.pending_n(), 1);
        // 窗口到 → tick 上报，延迟 ≤ 上限。
        let b = c.tick(150).unwrap();
        assert_eq!(b.count, 1);
        assert!(b.max_age_us <= MAX_LATENCY_US);
        assert_eq!(c.pending_n(), 0);
        assert_eq!(c.interrupts_out, 1);
    }

    #[test]
    fn latency_hard_cap_enforced() {
        let mut c = Coalescer::new();
        // 窗口被外部拉大也不许超上限：构造持续小步流（每 100μs），
        // 窗口上限 1000μs，延迟上限 2000μs——最老事件最多等 ~1000。
        let mut t = 0u64;
        for i in 0..50 {
            if let Some(b) = c.push(Ev { kind: 1, at_us: t }) {
                assert!(b.max_age_us <= MAX_LATENCY_US, "i={} age={}", i, b.max_age_us);
            }
            t += 100;
        }
        assert!(c.worst_wait() <= MAX_LATENCY_US);
    }

    #[test]
    fn window_adapts_both_directions() {
        let mut c = Coalescer::new();
        assert_eq!(c.window_us(), MIN_WINDOW_US);
        // 密集：gap=10μs << window → grow。
        let mut t = 0u64;
        for _ in 0..50 {
            let _ = c.push(Ev { kind: 1, at_us: t });
            t += 10;
        }
        assert!(c.window_us() > MIN_WINDOW_US);
        assert!(c.window_grew > 0);
        // 稀疏：gap=50000 > window → shrink。
        for _ in 0..50 {
            t += 50_000;
            let _ = c.push(Ev { kind: 1, at_us: t });
        }
        assert!(c.window_shrank > 0);
        // 最终落到下限（多次 shrink 到边界钳制）。
        for _ in 0..50 {
            t += 50_000;
            let _ = c.push(Ev { kind: 1, at_us: t });
        }
        assert_eq!(c.window_us(), MIN_WINDOW_US);
    }

    #[test]
    fn batch_full_reports_immediately() {
        let mut c = Coalescer::new();
        let mut reported = None;
        for i in 0..MAX_BATCH as u64 {
            reported = c.push(Ev { kind: 9, at_us: i });
        }
        assert!(reported.is_none(), "MAX_BATCH 个正好装满（未溢出）");
        // 第 MAX_BATCH+1 个触发立即上报。
        let b = c.push(Ev { kind: 9, at_us: MAX_BATCH as u64 }).unwrap();
        assert_eq!(b.count, MAX_BATCH);
        // 新批从当前事件重新开始。
        assert_eq!(c.pending_n(), 1);
    }

    #[test]
    fn accounting_never_leaks() {
        let mut c = Coalescer::new();
        let mut t = 0u64;
        let mut delivered = 0u64;
        let mut step = 1u64;
        for _ in 0..1000 {
            if let Some(b) = c.push(Ev { kind: 3, at_us: t }) {
                delivered += b.count as u64;
            }
            step = (step * 7 + 3) % 977;
            t += step;
        }
        // 收尾抽干。
        while let Some(b) = c.tick(t) {
            delivered += b.count as u64;
            t += MAX_LATENCY_US;
        }
        assert_eq!(delivered + c.pending_n() as u64, c.events_in);
        assert_eq!(c.events_in, 1000);
        // 中断数 < 事件数（合并发生了）。
        assert!(c.interrupts_out < c.events_in);
    }
}
