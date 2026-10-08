//! F183 磁盘健康 · 批次七深化（v7）——IO 错误率追踪、NVMe SMART 换算层、
//! 预测失效日（线性拟合）、维护窗口调度器。零堆、no_std。

use crate::checks::CheckSet;

/// IO 错误率窗口容量（最近 N 个千次采样桶）。
pub const IO_WINDOW_BUCKETS: usize = 16;
/// 错误率黄线（‰——千次 IO 中 5 次错）。
pub const IO_ERR_WARN_PERMILLE: u32 = 5;
/// 错误率红线。
pub const IO_ERR_CRIT_PERMILLE: u32 = 20;
/// 预测拟合最小样本。
pub const PREDICT_MIN_SAMPLES: usize = 4;
/// 维护窗口默认时长（分钟）。
pub const MAINT_WINDOW_MIN: u32 = 30;

/// IO 错误率追踪：按桶累计 (总操作, 错误)，滑窗比率与分级。
#[derive(Clone, Copy)]
pub struct IoErrorTracker {
    ops: [u32; IO_WINDOW_BUCKETS],
    errs: [u32; IO_WINDOW_BUCKETS],
    head: usize,
    pub n: usize,
}

impl IoErrorTracker {
    pub const fn new() -> IoErrorTracker {
        IoErrorTracker { ops: [0; IO_WINDOW_BUCKETS], errs: [0; IO_WINDOW_BUCKETS], head: 0, n: 0 }
    }

    /// 记录一桶（ops 上限 1000——桶定义是千次）。
    pub fn record_bucket(&mut self, ops: u32, errs: u32) {
        if ops > 1_000 {
            return; // 超桶定义拒收（口径纪律）
        }
        if self.n < IO_WINDOW_BUCKETS {
            self.ops[self.head] = ops;
            self.errs[self.head] = errs.min(ops);
            self.head = (self.head + 1) % IO_WINDOW_BUCKETS;
            self.n += 1;
        } else {
            self.ops[self.head] = ops;
            self.errs[self.head] = errs.min(ops);
            self.head = (self.head + 1) % IO_WINDOW_BUCKETS;
        }
    }

    /// 窗口错误率 ‰（u128 中间量防溢出）。
    pub fn error_rate_permille(&self) -> Option<u32> {
        let mut top = 0u128;
        let mut bad = 0u128;
        for i in 0..self.n {
            top += self.ops[i] as u128;
            bad += self.errs[i] as u128;
        }
        if top == 0 {
            return None;
        }
        Some(((bad * 1_000) / top) as u32)
    }

    /// 分级：None=无数据、0=绿、1=黄、2=红（与主层 Band 三段同构）。
    pub fn grade(&self) -> Option<u8> {
        let r = self.error_rate_permille()?;
        Some(if r >= IO_ERR_CRIT_PERMILLE {
            2
        } else if r >= IO_ERR_WARN_PERMILLE {
            1
        } else {
            0
        })
    }

    /// 窗口总操作数（对账面）。
    pub fn total_ops(&self) -> u128 {
        let mut t = 0u128;
        for i in 0..self.n {
            t += self.ops[i] as u128;
        }
        t
    }
}

/// NVMe SMART 换算层：原始 128bit 计数（这里 u64 足够模型面）→ 归一 ‰。
/// NVMe 健康页关键项：可预留剩余比例、可用备用块比例、介质错误数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NvmeHealth {
    /// 可用备用空间剩余 ‰（NVMe avail_spare 直接给百分比 → ×10）。
    pub spare_permille: u32,
    /// 介质错误计数（媒体数据完整性错误的累计）。
    pub media_errors: u64,
    /// 安全余量阈值 ‰（低于此线告警——厂商给定，模型取 100）。
    pub spare_thresh_permille: u32,
}

impl NvmeHealth {
    pub fn healthy(&self) -> bool {
        self.spare_permille >= self.spare_thresh_permille && self.media_errors == 0
    }

    /// 降级等级：备用低于阈值 = 1（黄）；有介质错误 = 2（红）；双坏 = 3。
    pub fn degrade_level(&self) -> u8 {
        let low_spare = self.spare_permille < self.spare_thresh_permille;
        let errs = self.media_errors > 0;
        match (low_spare, errs) {
            (false, false) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (true, true) => 3,
        }
    }
}

/// ATA SMART 197（当前待映射扇区）/198（不可修正扇区）的语义桥：
/// NVMe 层没有直接等价——换算规则：media_errors>0 ↔ 198>0（红级）。
pub fn ata198_equivalent(nv: &NvmeHealth) -> u64 {
    nv.media_errors
}

/// 预测失效日：按备用空间消耗率线性外推（样本 = (天数, 剩余‰)）。
/// 零堆：定点小数拟合（斜率 = 首末差/跨度——两点稳健估计）。
#[derive(Clone, Copy)]
pub struct FailurePredictor {
    days: [u32; 8],
    spare: [u32; 8],
    pub n: usize,
}

impl FailurePredictor {
    pub const fn new() -> FailurePredictor {
        FailurePredictor { days: [0; 8], spare: [0; 8], n: 0 }
    }

    pub fn push(&mut self, day: u32, spare_permille: u32) {
        if self.n < 8 {
            self.days[self.n] = day;
            self.spare[self.n] = spare_permille;
            self.n += 1;
        }
    }

    /// 每日消耗 ‰（首末两点斜率，向上取整——保守估计早报不晚报）。
    pub fn daily_burn_permille(&self) -> Option<u32> {
        if self.n < PREDICT_MIN_SAMPLES {
            return None;
        }
        let d0 = self.days[0];
        let d1 = self.days[self.n - 1];
        if d1 <= d0 {
            return None;
        }
        let s0 = self.spare[0] as u64;
        let s1 = self.spare[self.n - 1] as u64;
        if s1 >= s0 {
            return Some(0); // 备用没有在消耗（新盘/换盘）——不预测
        }
        Some(((s0 - s1) + (d1 - d0) as u64 - 1) as u32 / (d1 - d0) as u32)
    }

    /// 预测剩余天数（当前备用 ÷ 日耗；斜率为 0 → None 不编造）。
    pub fn days_remaining(&self) -> Option<u32> {
        let burn = self.daily_burn_permille()?;
        if burn == 0 {
            return None;
        }
        let cur = self.spare[self.n - 1] as u64;
        Some((cur / burn as u64) as u32)
    }
}

/// 维护窗口调度器：找下一个不撞忙时的整点窗口。
/// 忙时段表：24 bit 位图（1 = 忙）。
#[derive(Clone, Copy)]
pub struct MaintScheduler {
    busy_bitmap: u32,
    pub window_min: u32,
}

impl MaintScheduler {
    pub fn new(busy_bitmap: u32) -> MaintScheduler {
        MaintScheduler { busy_bitmap, window_min: MAINT_WINDOW_MIN }
    }

    /// 下一空闲整点（从 after_hour 起找，24 回绕；全忙 → None）。
    pub fn next_free_hour(&self, after_hour: u32) -> Option<u32> {
        if self.busy_bitmap == 0xFFFF_FFFF {
            return None;
        }
        for k in 1..=24u32 {
            let h = (after_hour + k) % 24;
            if self.busy_bitmap & (1 << h) == 0 {
                return Some(h);
            }
        }
        None
    }

    /// 窗口 [start, start+len) 是否全空闲（跨 0 点回绕判定）。
    pub fn window_free(&self, start_hour: u32, len_hours: u32) -> bool {
        for k in 0..len_hours {
            let h = (start_hour + k) % 24;
            if self.busy_bitmap & (1 << h) != 0 {
                return false;
            }
        }
        true
    }

    /// 调度：从 after_hour 找能容纳窗口的起点（窗口按小时粒度）。
    pub fn schedule(&self, after_hour: u32, len_hours: u32) -> Option<u32> {
        for k in 1..=24u32 {
            let h = (after_hour + k) % 24;
            if self.window_free(h, len_hours) {
                return Some(h);
            }
        }
        None
    }
}

#[inline(never)]
pub fn run_diskhealth_b7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F183-b7");

    // 1) 错误率计算：3 桶 1000/5/1000/10/1000/0 → 15/3000 = 5‰（恰黄线）。
    let mut t = IoErrorTracker::new();
    t.record_bucket(1_000, 5);
    t.record_bucket(1_000, 10);
    t.record_bucket(1_000, 0);
    cs.add("io_rate_compute", t.error_rate_permille() == Some(5) && t.total_ops() == 3_000, "");

    // 2) 分级三段：0‰ 绿、5‰ 黄、20‰ 红（线值恰点归属高档）。
    let mut g = IoErrorTracker::new();
    g.record_bucket(1_000, 0);
    let mut y = IoErrorTracker::new();
    y.record_bucket(1_000, 5);
    let mut r = IoErrorTracker::new();
    r.record_bucket(1_000, 20);
    cs.add(
        "io_grade_three_bands",
        g.grade() == Some(0) && y.grade() == Some(1) && r.grade() == Some(2),
        "",
    );

    // 3) 空窗诚实：零数据 → None（不编造 0‰ 健康）。
    cs.add("io_empty_honest", IoErrorTracker::new().error_rate_permille().is_none(), "");

    // 4) 超桶拒收与错误钳制：ops>1000 拒、errs>ops 钳到 ops（口径纪律）。
    let mut t2 = IoErrorTracker::new();
    t2.record_bucket(1_001, 0);
    t2.record_bucket(100, 500);
    cs.add(
        "io_bucket_guards",
        t2.total_ops() == 100 && t2.error_rate_permille() == Some(1_000),
        "",
    );

    // 5) 环回卷：17 桶 → 只留最近 16 桶（第一桶出账）。
    let mut t3 = IoErrorTracker::new();
    for i in 0..17u32 {
        t3.record_bucket(1_000, i);
    }
    cs.add(
        "io_ring_wraps",
        t3.n == IO_WINDOW_BUCKETS && t3.total_ops() == 16_000 && t3.error_rate_permille() == Some(8),
        "",
    );

    // 6) NVMe 健康判定：备用足+零介质错 = 健康；单低备用 = 1；单介质错 = 2；双坏 = 3。
    let nv_ok = NvmeHealth { spare_permille: 500, media_errors: 0, spare_thresh_permille: 100 };
    let nv_spare = NvmeHealth { spare_permille: 90, media_errors: 0, spare_thresh_permille: 100 };
    let nv_err = NvmeHealth { spare_permille: 500, media_errors: 3, spare_thresh_permille: 100 };
    let nv_both = NvmeHealth { spare_permille: 50, media_errors: 7, spare_thresh_permille: 100 };
    cs.add(
        "nvme_degrade_levels",
        nv_ok.healthy() && nv_ok.degrade_level() == 0 && nv_spare.degrade_level() == 1 && nv_err.degrade_level() == 2 && nv_both.degrade_level() == 3,
        "",
    );

    // 7) ATA↔NVMe 桥：198 等价量 = 介质错误计数（语义一一映射不另造）。
    cs.add("ata_bridge_mapping", ata198_equivalent(&nv_err) == 3 && ata198_equivalent(&nv_ok) == 0, "");

    // 8) 预测：样本不足 4 → None 不猜；样本够 → 日耗向上取整（保守早报）。
    let p = FailurePredictor::new();
    cs.add("predict_insufficient", p.days_remaining().is_none(), "");
    let mut p2 = FailurePredictor::new();
    // 0 天 900‰ → 30 天 600‰：日耗 10‰ → 剩 60 天。
    p2.push(0, 900);
    p2.push(10, 870);
    p2.push(20, 830);
    p2.push(30, 600);
    cs.add(
        "predict_burn_and_remaining",
        p2.daily_burn_permille() == Some(10) && p2.days_remaining() == Some(60),
        "",
    );

    // 9) 备用不降不预测：备用在涨（换新盘）→ burn=0 → days None（不编造终局）。
    let mut p3 = FailurePredictor::new();
    p3.push(0, 100);
    p3.push(10, 200);
    p3.push(20, 300);
    p3.push(30, 500);
    cs.add("predict_rising_no_forecast", p3.daily_burn_permille() == Some(0) && p3.days_remaining().is_none(), "");

    // 10) 调度：忙位图避开 → 下一空闲整点；全忙 → None（24 小时无窗诚实上报）。
    let busy = 0b0000_0000_0011_1111u32; // 0..5 点忙
    let s = MaintScheduler::new(busy);
    let all_busy = MaintScheduler::new(0xFFFF_FFFF);
    cs.add(
        "maint_next_free",
        s.next_free_hour(0) == Some(6) && s.next_free_hour(23) == Some(6) && all_busy.next_free_hour(0).is_none(),
        "",
    );

    // 11) 窗口判定跨 0 点：23 点起 2 小时含 0 点——0 忙则窗不空。
    let night_busy = 1u32 << 0; // 0 点忙
    let s2 = MaintScheduler::new(night_busy);
    cs.add(
        "maint_window_wraps",
        s2.window_free(23, 2) == false && s2.window_free(22, 2) == true,
        "",
    );

    // 12) 完整调度：2 小时窗从 6 点起找 → 6（6、7 点都闲）。
    cs.add("maint_schedule_two_hours", s.schedule(0, 2) == Some(6), "");

    // 13) 常量自洽：黄<红、桶 16、窗 30 分钟。
    cs.add(
        "io_constants",
        IO_ERR_WARN_PERMILLE < IO_ERR_CRIT_PERMILLE && IO_WINDOW_BUCKETS == 16 && MAINT_WINDOW_MIN == 30,
        "",
    );

    // 14) 桶错误钳制不变量：errs ≤ ops 恒成立（record 后回读验证）。
    let mut t4 = IoErrorTracker::new();
    t4.record_bucket(50, 999);
    cs.add("io_errs_clamped", t4.error_rate_permille() == Some(1_000), "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predictor_ceil_favors_early_warning() {
        // 整除不尽时向上取整（例如 5‰ 余量 12 → 3 天而非 2 天——早报）。
        let mut p = FailurePredictor::new();
        p.push(0, 900);
        p.push(10, 800);
        p.push(20, 710);
        p.push(30, 610);
        let burn = p.daily_burn_permille().unwrap();
        let days = p.days_remaining().unwrap();
        // 610 / burn，burn = ceil((900-610)/30) = ceil(9.67) = 10 → 61 天。
        assert_eq!(burn, 10);
        assert_eq!(days, 61);
    }

    #[test]
    fn io_rate_large_ops_no_overflow() {
        // 大操作量不溢出：16 桶全 1000、错误全 1000 → 1000‰ 恒定。
        let mut t = IoErrorTracker::new();
        for _ in 0..IO_WINDOW_BUCKETS {
            t.record_bucket(1_000, 1_000);
        }
        assert_eq!(t.error_rate_permille(), Some(1_000));
    }

    #[test]
    fn scheduler_full_cycle() {
        // 24 小时全忙逐位验证：任何起点 next_free 都是 None。
        for h in 0..24u32 {
            assert!(MaintScheduler::new(0xFFFF_FFFF).next_free_hour(h).is_none());
        }
    }
}
