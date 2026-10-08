//! mech_scan — 水位均衡回收循环 + 空闲清零细流节流（AI-K1 深化批次四 · F045/F049）。
//!
//! 主册依据：
//! - F045【设计细节】「断电百次中脏页丢失窗口 ≤ 水位规则承诺值」——水位
//!   规则的**执行循环**（kswapd 同族：min/low/high 水位 + 优先级递进扫描）
//!   此前只有档位表与曲线；本件给均衡器本体：低于 low 启动、达 high 收工、
//!   priority 0..10 递进（扫描努力指数上升）、回收进度记账、拥塞等待。
//! - F049【设计细节】「空转页清零工程」——清零的**节流治理**：空闲预算
//!   （每秒毫秒级）内的批量调度，busy 信号 armed 即停（F049 fence 语义），
//!   预算账目诚实（用过多少、停了多少，可问）。
//! - 两者合成"后台功耗安全"的一对：一个管回收节奏，一个管清零节奏。
//! - 零堆、零浮点。

// ---------------------------------------------------------------------------
// 1. 水位均衡回收循环（kswapd 同族）
// ---------------------------------------------------------------------------

/// 每区页数（模拟区；消费域按真实区表实例化）。
pub const ZONE_PAGES: u64 = 1 << 20; // 4GB / 4KB

/// 优先级上限（0 = 最努力；10 = 放弃线——Linux PFRLIMIT 同形）。
pub const MIN_PRIORITY: u8 = 10;

/// 单次扫描一批页数（priority 越低批越大：batch = base << (10-priority)/2）。
pub const SCAN_BASE: u64 = 32;

#[derive(Clone, Copy, Debug)]
pub struct Zone {
    pub free: u64,
    pub min: u64,
    pub low: u64,
    pub high: u64,
}

impl Zone {
    pub const fn new(free: u64, min: u64, low: u64, high: u64) -> Self {
        Zone { free, min, low, high }
    }

    pub fn needs_reclaim(&self) -> bool {
        self.free < self.low
    }

    pub fn saturated(&self) -> bool {
        self.free >= self.high
    }
}

/// 回收结果账（一回合）。
#[derive(Clone, Copy, Debug, Default)]
pub struct ReclaimReport {
    pub priority_reached: u8,
    pub scanned: u64,
    pub reclaimed: u64,
    pub congestion_waits: u32,
    pub rounds: u32,
    pub gave_up: bool,
}

/// 均衡循环：free < low → 逐级加压回收直到 ≥ high 或放弃线。
/// `reclaim_fn(page) -> bool`：回收一页是否成功（消费域注入真实策略——
/// 干净页直接丢、脏页触发写合并（F046 通道）等）。
pub fn balance_zone<F>(z: &mut Zone, mut reclaim_fn: F, max_rounds: u32) -> ReclaimReport
where
    F: FnMut(u64) -> bool,
{
    let mut rep = ReclaimReport::default();
    if !z.needs_reclaim() {
        return rep;
    }
    let mut priority: u8 = 3; // 常规起点（Linux KSWAPD_START 同形）
    while !z.saturated() && rep.rounds < max_rounds {
        rep.rounds += 1;
        let batch = SCAN_BASE << ((MIN_PRIORITY - priority) / 2).min(4);
        let target = z.high - z.free;
        for _ in 0..batch.min(target.max(1)) {
            rep.scanned += 1;
            if reclaim_fn(rep.scanned) {
                rep.reclaimed += 1;
                z.free += 1;
            }
        }
        // 进度不足 → 优先级递进（更努力）。
        if !z.saturated() && priority > 0 {
            priority -= 1;
        } else if !z.saturated() && priority == 0 {
            // 最努力仍不足 → 拥塞等待（记账，不空转）。
            rep.congestion_waits += 1;
        }
    }
    rep.priority_reached = priority;
    rep.gave_up = !z.saturated() && rep.rounds >= max_rounds;
    rep
}

// ---------------------------------------------------------------------------
// 2. 空闲清零细流节流（F049）
// ---------------------------------------------------------------------------

/// 每秒空闲预算（μs）：主册 F049 口径「静置 60 秒合成器 CPU <0.5%」反推——
/// 清零线程预算 2ms/s = 0.2%，含内核其余闲时工作仍 <0.5% 线。
pub const ZERO_BUDGET_PER_S_US: u64 = 2000;
/// 单批清零页数（批间再评估 armed——细到可中断）。
pub const ZERO_BATCH_PAGES: u64 = 16;

/// 细流清零治理器。
#[derive(Clone, Copy, Debug)]
pub struct TrickleZero {
    /// 本秒已用预算（μs）。
    used_us_this_sec: u64,
    cur_sec: u64,
    pub armed_pause: bool,
    /// 生命周期账。
    pub pages_zeroed: u64,
    pub batches: u64,
    pub budget_paused_us: u64,
}

impl TrickleZero {
    pub const fn new() -> Self {
        TrickleZero {
            used_us_this_sec: 0,
            cur_sec: 0,
            armed_pause: false,
            pages_zeroed: 0,
            batches: 0,
            budget_paused_us: 0,
        }
    }

    /// busy 信号（F049 fence：光标闪烁/输入活动 armed → 立即停）。
    pub fn set_armed(&mut self, armed: bool) {
        self.armed_pause = armed;
    }

    /// 请求一批清零。返回本批页数（0 = 本秒预算尽 / busy armed）。
    pub fn request_batch(&mut self, now_us: u64, batch_cost_us_per_page: u64) -> u64 {
        let sec = now_us / 1_000_000;
        if sec != self.cur_sec {
            // 跨秒：上一秒被 armed 暂停的剩余预算如实记账（不是丢弃）。
            self.budget_paused_us += ZERO_BUDGET_PER_S_US.saturating_sub(self.used_us_this_sec);
            self.cur_sec = sec;
            self.used_us_this_sec = 0;
        }
        if self.armed_pause {
            return 0;
        }
        if self.used_us_this_sec >= ZERO_BUDGET_PER_S_US {
            return 0;
        }
        // 批量受剩余预算钳制（细到页——批内不再评估 armed 是诚实登记：
        // 16 页 @ ~1μs/页 = 16μs，最坏迟滞 16μs，远低于一帧）。
        let remain = ZERO_BUDGET_PER_S_US - self.used_us_this_sec;
        let pages = ZERO_BATCH_PAGES.min(remain / batch_cost_us_per_page.max(1));
        if pages == 0 {
            return 0;
        }
        self.used_us_this_sec += pages * batch_cost_us_per_page;
        self.pages_zeroed += pages;
        self.batches += 1;
        pages
    }

    /// 本秒剩余预算（可观测——"清零线程现在还能干多少"）。
    pub fn remain_us_this_sec(&self) -> u64 {
        ZERO_BUDGET_PER_S_US.saturating_sub(self.used_us_this_sec)
    }
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// CheckSet（挂 F045）
// ---------------------------------------------------------------------------

/// 运行检查项（判据锚点见对账表批次四段）。
use crate::checks::CheckSet;

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F045-mech-scan");
    // 1) 水位均衡：低于 low 启动、达 high 收工。
    let mut z = Zone::new(150, 100, 200, 400);
    let mut next_page = 0u64;
    let rep = balance_zone(&mut z, |_| {
        next_page += 1;
        next_page % 3 == 0
    }, 1000);
    cs.add("balance_saturates", z.saturated() && rep.reclaimed >= 250 && !rep.gave_up, "");
    // 2) 细流清零：预算上沿 + 用满。
    let mut t = TrickleZero::new();
    let mut total = 0u64;
    for ms in 0..1000u64 {
        total += t.request_batch(ms * 1000, 1);
    }
    cs.add(
        "trickle_budget",
        total <= ZERO_BUDGET_PER_S_US && total >= ZERO_BUDGET_PER_S_US - ZERO_BATCH_PAGES,
        "",
    );
    // 3) armed 立即停 / 解除恢复 / 剩余预算钳制。
    let mut t2 = TrickleZero::new();
    t2.set_armed(true);
    let stopped = t2.request_batch(0, 1) == 0;
    t2.set_armed(false);
    let resumed = t2.request_batch(1000, 1) > 0;
    let clamped = t2.request_batch(0, 300) == 6;
    cs.add("armed_stop_and_clamp", stopped && resumed && clamped, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balance_escalates_and_saturates() {
        // 每三页可回收一页（脏页写回代价模型）；free=150 < low=200 触发。
        let mut z = Zone::new(150, 100, 200, 400);
        let mut next_page = 0u64;
        let rep = balance_zone(&mut z, |_| {
            next_page += 1;
            next_page % 3 == 0
        }, 1000);
        assert!(z.saturated(), "回收必须到达 high 水位：free={}", z.free);
        assert!(rep.reclaimed >= 250);
        assert!(!rep.gave_up);
    }

    #[test]
    fn balance_noop_above_low() {
        let mut z = Zone::new(5000, 100, 200, 400);
        let rep = balance_zone(&mut z, |_| true, 100);
        assert_eq!((rep.rounds, rep.scanned, rep.reclaimed), (0, 0, 0), "水位之上不动作");
    }

    #[test]
    fn balance_gives_up_honestly() {
        // 一页都收不回 → 优先级递进到 0、拥塞记账、gave_up 如实。
        let mut z = Zone::new(150, 50, 200, 400);
        let rep = balance_zone(&mut z, |_| false, 50);
        assert!(rep.gave_up);
        assert_eq!(rep.reclaimed, 0);
        assert_eq!(rep.rounds, 50, "max_rounds 是硬上限");
    }

    #[test]
    fn trickle_respects_budget() {
        let mut t = TrickleZero::new();
        // 1μs/页：预算 2000μs → 一秒内最多 2000 页。
        let mut total = 0u64;
        for ms in 0..1000u64 {
            total += t.request_batch(ms * 1000, 1);
        }
        assert!(total <= ZERO_BUDGET_PER_S_US, "预算上沿必须成立：{}", total);
        assert!(total >= ZERO_BUDGET_PER_S_US - ZERO_BATCH_PAGES, "预算必须被用满（细流不是摆设）");
    }

    #[test]
    fn trickle_stops_on_armed_and_resumes() {
        let mut t = TrickleZero::new();
        t.set_armed(true);
        assert_eq!(t.request_batch(0, 1), 0, "busy armed 立即停");
        t.set_armed(false);
        assert!(t.request_batch(1000, 1) > 0, "解除后恢复");
    }

    #[test]
    fn trickle_batch_clamped_by_remaining_budget() {
        let mut t = TrickleZero::new();
        // 每页 300μs：预算 2000 → 首批 6 页（1800），次批只剩 200μs → 0 页。
        assert_eq!(t.request_batch(0, 300), 6);
        assert_eq!(t.request_batch(1000, 300), 0);
        assert_eq!(t.remain_us_this_sec(), 200);
        // 跨秒重置。
        assert_eq!(t.request_batch(1_000_001, 300), 6);
    }

    #[test]
    fn trickle_pause_bookkeeping_is_honest() {
        let mut t = TrickleZero::new();
        t.set_armed(true);
        let _ = t.request_batch(0, 1);
        // 跨秒：整秒预算都在 armed 暂停 → 记入 budget_paused_us。
        let _ = t.request_batch(1_500_000, 1);
        assert!(t.budget_paused_us >= ZERO_BUDGET_PER_S_US, "暂停的预算必须入账而非消失");
    }
}
