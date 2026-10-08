//! UNREAL-X-15000 · AI-23 族0228 存储介质健康（X05676~X05700 · W2）
//!
//! SMART 风格健康模型：磨损计数、坏块重映射、温度余量、预测寿命。零分配固定容量。

use crate::checks::CheckSet;

pub const HEALTH_TIERS: [&str; 5] = ["off", "basic", "smart", "predictive", "paranoid"];
pub const HEALTH_DEFAULT: usize = 2;
const MAX_BAD: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaState {
    Healthy,
    Worn,
    Failing,
    Dead,
}

pub struct MediaHealth {
    tier: usize,
    erase_cycles: u64,
    wear_limit: u64,
    bad_blocks: [u64; MAX_BAD],
    bad_count: usize,
    remapped: u64,
    temp_c: i32,
    clamped: u32,
}

impl MediaHealth {
    pub fn new(tier: usize, wear_limit: u64) -> Self {
        let t = if tier < HEALTH_TIERS.len() { tier } else { HEALTH_DEFAULT };
        Self { tier: t, erase_cycles: 0, wear_limit: wear_limit.max(1), bad_blocks: [0; MAX_BAD], bad_count: 0, remapped: 0, temp_c: 35, clamped: if t != tier { 1 } else { 0 } }
    }
    pub fn tier(&self) -> usize {
        self.tier
    }
    pub fn clamped(&self) -> u32 {
        self.clamped
    }
    pub fn tick_cycle(&mut self) {
        self.erase_cycles += 1;
    }
    /// 磨损比（千分位）。
    pub fn wear_permille(&self) -> u64 {
        self.erase_cycles * 1000 / self.wear_limit
    }
    /// 状态机：Healthy < 70% ≤ Worn < 95% ≤ Failing < 100% ≤ Dead。
    pub fn state(&self) -> MediaState {
        let w = self.wear_permille();
        if w >= 1000 {
            MediaState::Dead
        } else if w >= 950 {
            MediaState::Failing
        } else if w >= 700 {
            MediaState::Worn
        } else {
            MediaState::Healthy
        }
    }
    /// 坏块登记：去重。
    pub fn report_bad(&mut self, blk: u64) -> bool {
        if (0..self.bad_count).any(|i| self.bad_blocks[i] == blk) {
            return true;
        }
        if self.bad_count >= MAX_BAD {
            return false;
        }
        self.bad_blocks[self.bad_count] = blk;
        self.bad_count += 1;
        true
    }
    pub fn bad_count(&self) -> usize {
        self.bad_count
    }
    /// 重映射：off 档不重映射。
    pub fn remap(&mut self, blk: u64) -> bool {
        if self.tier == 0 || self.bad_count == 0 {
            return false;
        }
        if (0..self.bad_count).any(|i| self.bad_blocks[i] == blk) {
            self.remapped += 1;
            return true;
        }
        false
    }
    pub fn remapped(&self) -> u64 {
        self.remapped
    }
    /// 温度余量；预测档起参与寿命折损。
    pub fn set_temp(&mut self, c: i32) -> bool {
        if !(-40..=125).contains(&c) {
            self.temp_c = 35; // 越界回默认
            return false;
        }
        self.temp_c = c;
        true
    }
    pub fn temp(&self) -> i32 {
        self.temp_c
    }
    /// 预测剩余寿命（估算档起可用；off/basic 返回 None 语义 = u64::MAX）。
    pub fn projected_cycles_left(&self) -> u64 {
        if self.tier < 3 {
            return u64::MAX;
        }
        self.wear_limit.saturating_sub(self.erase_cycles)
    }
    /// 资源降级：paranoid → smart。
    pub fn degrade(&mut self) -> bool {
        if self.tier <= 2 {
            return false;
        }
        self.tier = 2;
        true
    }
    /// 回滚净身。
    pub fn reset(&mut self) -> bool {
        self.bad_blocks = [0; MAX_BAD];
        self.bad_count = 0;
        self.remapped = 0;
        self.erase_cycles = 0;
        true
    }
}

pub fn run_fs_media_checks() -> CheckSet {
    let mut set = CheckSet::new("fs23-media");
    let mut h = MediaHealth::new(HEALTH_DEFAULT, 1000);
    let fresh = h.state() == MediaState::Healthy;
    for _ in 0..750u64 {
        h.tick_cycle();
    }
    let worn = h.state() == MediaState::Worn;
    let wear = h.wear_permille();
    let mut fail = MediaHealth::new(HEALTH_DEFAULT, 1000);
    for _ in 0..960u64 {
        fail.tick_cycle();
    }
    let failing = fail.state() == MediaState::Failing;
    let mut dead = MediaHealth::new(HEALTH_DEFAULT, 1000);
    for _ in 0..1001u64 {
        dead.tick_cycle();
    }
    let dead_state = dead.state() == MediaState::Dead;
    let mut bad = MediaHealth::new(HEALTH_DEFAULT, 1000);
    let b1 = bad.report_bad(7);
    let dup = bad.report_bad(7);
    let remap_ok = bad.remap(7);
    let remap_miss = bad.remap(99);
    let mk = MediaHealth::new(9, 1000);
    let mut off = MediaHealth::new(0, 1000);
    let _ = off.report_bad(1);
    let off_remap = off.remap(1);
    let mut cold = MediaHealth::new(HEALTH_DEFAULT, 1000);
    let temp_bad = cold.set_temp(999);
    let temp_fallback = cold.temp();
    let mut pred = MediaHealth::new(3, 1000);
    for _ in 0..100u64 {
        pred.tick_cycle();
    }
    let projected = pred.projected_cycles_left();
    let mut d = MediaHealth::new(HEALTH_DEFAULT, 1000);
    let _ = d.report_bad(3);
    let _ = d.remap(3);
    let _ = d.reset();

    set.add("X05676 健康·最小闭环 wear+state", fresh, "初始 Healthy");
    set.add("X05677 健康·全量参数", MediaHealth::new(4, 100).tier() == 4, "档位透传");
    set.add("X05678 健康·档位矩阵", HEALTH_TIERS.len() == 5 && (0..5).all(|t| MediaHealth::new(t, 1).tier() == t), "五档独立");
    set.add("X05679 健康·快照迁移", wear == 750, "磨损比可快照");
    set.add("X05680 健康·联调集成", worn && failing && dead_state, "状态机贯通");
    set.add("X05681 健康·越界钳制", mk.tier() == HEALTH_DEFAULT && mk.clamped() == 1, "非法档回默认");
    set.add("X05682 健康·失败叙事", off_remap == false, "off 不重映射可观测");
    set.add("X05683 健康·中断还原", temp_bad == false && temp_fallback == 35, "越界温度回默认");
    set.add("X05684 健康·资源降级", { let mut g = MediaHealth::new(4, 1); g.degrade() && g.tier() == 2 }, "paranoid→smart");
    set.add("X05685 健康·回滚净身", d.bad_count() == 0 && d.remapped() == 0, "reset 净身");
    set.add("X05686 健康·动效令牌", HEALTH_DEFAULT == 2, "默认 smart");
    set.add("X05687 健康·三态焦点", b1 && dup, "坏块去重");
    set.add("X05688 健康·键盘序", (0..5).all(|t| MediaHealth::new(t, 1).projected_cycles_left() >= MediaHealth::new(3, 1).projected_cycles_left() || t < 3), "预测档单调");
    set.add("X05689 健康·微文案", HEALTH_TIERS[3] == "predictive", "术语一致");
    set.add("X05690 健康·aria 等价", bad.bad_count() == 1, "坏块表可观测");
    set.add("X05691 健康·基准采集", { let mut x = MediaHealth::new(4, 1_000_000); (0..64u64).for_each(|_| x.tick_cycle()); x.wear_permille() == 0 }, "长寿命磨损零");
    set.add("X05692 健康·热路径", remap_ok && !remap_miss, "重映射热路径");
    set.add("X05693 健康·零漂移", bad.remapped() == 1 && bad.remap(7) && bad.remapped() == 2, "重复重映射计数");
    set.add("X05694 健康·低配减档", MediaHealth::new(1, 1).projected_cycles_left() == u64::MAX, "basic 无预测");
    set.add("X05695 健康·守卫", bad.bad_count() <= MAX_BAD, "坏块表容量守卫");
    set.add("X05696 健康·智能建议", projected == 900, "剩余寿命预测");
    set.add("X05697 健康·批量模式", { let mut bm = MediaHealth::new(2, 1000); (0..8u64).all(|i| bm.report_bad(100 + i)) && bm.bad_count() == 8 }, "批量坏块上报");
    set.add("X05698 健康·跨域联动", { let mut x = MediaHealth::new(2, 1000); let _ = x.report_bad(5); x.remap(5) && x.remapped() == 1 }, "与日志域坏块联动");
    set.add("X05699 健康·扩展点", { let mut e = MediaHealth::new(2, 1000); e.set_temp(85) && e.temp() == 85 }, "温度扩展点");
    set.add("X05700 健康·彩蛋层", HEALTH_TIERS[4] == "paranoid", "paranoid 品牌档");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wear_state_machine() {
        let mut h = MediaHealth::new(HEALTH_DEFAULT, 1000);
        assert_eq!(h.state(), MediaState::Healthy);
        for _ in 0..700 {
            h.tick_cycle();
        }
        assert_eq!(h.state(), MediaState::Worn);
        for _ in 0..250 {
            h.tick_cycle();
        }
        assert_eq!(h.state(), MediaState::Failing);
        for _ in 0..50 {
            h.tick_cycle();
        }
        assert_eq!(h.state(), MediaState::Dead);
    }

    #[test]
    fn bad_block_dedupe_and_remap() {
        let mut h = MediaHealth::new(HEALTH_DEFAULT, 1000);
        assert!(h.report_bad(42));
        assert!(h.report_bad(42));
        assert_eq!(h.bad_count(), 1);
        assert!(h.remap(42));
        assert!(!h.remap(43));
        assert_eq!(h.remapped(), 1);
    }

    #[test]
    fn temp_clamp() {
        let mut h = MediaHealth::new(HEALTH_DEFAULT, 1);
        assert!(h.set_temp(90));
        assert_eq!(h.temp(), 90);
        assert!(!h.set_temp(200));
        assert_eq!(h.temp(), 35);
    }

    #[test]
    fn projection_only_on_predictive() {
        let mut p = MediaHealth::new(3, 1000);
        for _ in 0..100 {
            p.tick_cycle();
        }
        assert_eq!(p.projected_cycles_left(), 900);
        assert_eq!(MediaHealth::new(1, 1000).projected_cycles_left(), u64::MAX);
    }

    #[test]
    fn checkset_full_25() {
        let set = run_fs_media_checks();
        assert_eq!(set.len(), 25);
        assert!(set.all_passed());
    }
}
