//! UNX-F56164 上下文切换对照聚合件（接管 F56025）（0x61480 段）
//!
//! 锚点：UNX-F56164-J1 上下文切换聚合账（切换四类——自愿/ preempt/
//! 迁移/中断——逐类计数与均值延迟落账：类 × 计数 × 均值延迟 × 指纹）；
//! J2 聚合语义（均值 = 总延迟/计数整数域，零计数均值 0 哨兵）；J3 接
//! 管对账印（F56025 接管声明 + 四类全 Green 才可印）三判据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·上下文切换面）。
//!
//! 结构：切换四类闭集 + 事件录入核（类闭集 + 延迟界）+ 聚合核（计数/
//! 总延迟/均值三值）+ 接管印（四类全绿 + 接管声明）+ 注入核（未知类/
//! 零计数均值误用/延迟越界/印后写/接管声明缺失必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61480 独占 0x20）。
pub const SEG_BASE: u32 = 0x61480;
pub const CS_UNK_KIND: u32 = SEG_BASE + 0x01; // 未知切换类
pub const CS_ZERO_MEAN: u32 = SEG_BASE + 0x02; // 零计数均值误用
pub const CS_LAT_RANGE: u32 = SEG_BASE + 0x03; // 延迟越界
pub const CS_SEALED_WRITE: u32 = SEG_BASE + 0x04; // 印后写
pub const CS_TAKEOVER_MISS: u32 = SEG_BASE + 0x05; // 接管声明缺失

/// 切换四类闭集。
pub const K_VOLUNTARY: usize = 0;
pub const K_PREEMPT: usize = 1;
pub const K_MIGRATE: usize = 2;
pub const K_INTERRUPT: usize = 3;
pub const KIND_COUNT: usize = 4;

/// 类名渲染。
pub const KIND_NAMES: [&str; KIND_COUNT] = ["voluntary", "preempt", "migrate", "interrupt"];

/// 接管声明锚（F56025 → 本件接管）。
pub const TAKEOVER_FROM: &str = "F56025";

/// 延迟界（微秒，≤ 1s）。
pub const LAT_MAX: u64 = 1_000_000;

/// 均值核：总延迟 / 计数，零计数 → 0 哨兵（调用方须先判计数）。
pub fn mean_latency(total: u64, count: u64) -> u64 {
    if count == 0 {
        return 0;
    }
    total / count
}

/// 聚合账（单类）。
#[derive(Clone)]
pub struct SwitchAgg {
    pub kind: usize,
    pub count: u64,
    pub total_lat: u64,
}

impl SwitchAgg {
    fn render_line(&self) -> String {
        format!(
            "{}|n{}|lat{}|mean{}",
            KIND_NAMES[self.kind],
            self.count,
            self.total_lat,
            mean_latency(self.total_lat, self.count)
        )
    }
}

/// 上下文切换聚合器。
pub struct CtxSwitchAggregator {
    aggs: [SwitchAgg; KIND_COUNT],
    sealed: bool,
    takeover_declared: bool,
}

impl CtxSwitchAggregator {
    pub fn new() -> Self {
        CtxSwitchAggregator {
            aggs: [
                SwitchAgg { kind: K_VOLUNTARY, count: 0, total_lat: 0 },
                SwitchAgg { kind: K_PREEMPT, count: 0, total_lat: 0 },
                SwitchAgg { kind: K_MIGRATE, count: 0, total_lat: 0 },
                SwitchAgg { kind: K_INTERRUPT, count: 0, total_lat: 0 },
            ],
            sealed: false,
            takeover_declared: false,
        }
    }

    /// 声明接管（印前置）。
    pub fn declare_takeover(&mut self) {
        self.takeover_declared = true;
    }

    /// 录一事件：类闭集 + 延迟界 + 印后拒。
    pub fn record(&mut self, kind: usize, lat_us: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(CS_SEALED_WRITE);
        }
        if kind >= KIND_COUNT {
            return Err(CS_UNK_KIND);
        }
        if lat_us > LAT_MAX {
            return Err(CS_LAT_RANGE);
        }
        self.aggs[kind].count += 1;
        self.aggs[kind].total_lat += lat_us;
        Ok(())
    }

    pub fn agg(&self, kind: usize) -> Option<&SwitchAgg> {
        self.aggs.get(kind)
    }

    /// 均值查询：零计数返回 Err（显式拒绝误用）。
    pub fn mean_of(&self, kind: usize) -> Result<u64, u32> {
        if kind >= KIND_COUNT {
            return Err(CS_UNK_KIND);
        }
        if self.aggs[kind].count == 0 {
            return Err(CS_ZERO_MEAN);
        }
        Ok(mean_latency(self.aggs[kind].total_lat, self.aggs[kind].count))
    }

    /// 接管印：接管声明 + 四类全有事件才可印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || !self.takeover_declared {
            return false;
        }
        if self.aggs.iter().any(|a| a.count == 0) {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 聚合指纹。
    pub fn agg_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for a in &self.aggs {
            h = h.wrapping_mul(0x0100_0193) ^ (a.count as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (a.total_lat as u32);
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 渲染导出。
    pub fn render_agg(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("TAKEOVER:{}\n", TAKEOVER_FROM));
        for a in &self.aggs {
            out.push_str(&a.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("CTXSW-SEAL\n");
        }
        out
    }
}

impl Default for CtxSwitchAggregator {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四类各录事件（voluntary 40 次 ×50μs / preempt 20×200 /
/// migrate 10×500 / interrupt 30×100）。
fn standard_agg() -> CtxSwitchAggregator {
    let mut a = CtxSwitchAggregator::new();
    a.declare_takeover();
    for _ in 0..40 {
        let _ = a.record(K_VOLUNTARY, 50);
    }
    for _ in 0..20 {
        let _ = a.record(K_PREEMPT, 200);
    }
    for _ in 0..10 {
        let _ = a.record(K_MIGRATE, 500);
    }
    for _ in 0..30 {
        let _ = a.record(K_INTERRUPT, 100);
    }
    a
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知类
    let mut a = CtxSwitchAggregator::new();
    if a.record(9, 100) == Err(CS_UNK_KIND) {
        caught += 1;
    }
    // 注 2：零计数均值误用
    let a = CtxSwitchAggregator::new();
    if a.mean_of(K_VOLUNTARY) == Err(CS_ZERO_MEAN) {
        caught += 1;
    }
    // 注 3：延迟越界
    let mut a = CtxSwitchAggregator::new();
    if a.record(K_PREEMPT, LAT_MAX + 1) == Err(CS_LAT_RANGE) {
        caught += 1;
    }
    // 注 4：缺接管声明印
    let mut a = CtxSwitchAggregator::new();
    for k in 0..KIND_COUNT {
        let _ = a.record(k, 100);
    }
    if !a.seal() && !a.sealed() {
        caught += 1;
    }
    // 注 5：印后写
    let mut a = standard_agg();
    let _ = a.seal();
    if a.record(K_VOLUNTARY, 1) == Err(CS_SEALED_WRITE) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 聚合账；J2 聚合语义；J3 接管印。
pub fn run_unx56164_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56164");
    // J1-1 四类计数齐
    let a = standard_agg();
    let ok = a.agg(K_VOLUNTARY).map(|x| x.count == 40).unwrap_or(false)
        && a.agg(K_PREEMPT).map(|x| x.count == 20).unwrap_or(false)
        && a.agg(K_MIGRATE).map(|x| x.count == 10).unwrap_or(false)
        && a.agg(K_INTERRUPT).map(|x| x.count == 30).unwrap_or(false);
    s.add("56164-J1-四类计数齐", ok, if ok { "" } else { "计数漂移" });
    // J1-2 渲染导出齐（含接管声明）
    let text = standard_agg().render_agg();
    let ok = text.contains("TAKEOVER:F56025") && text.contains("voluntary|n40|lat2000|mean50");
    s.add("56164-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 均值锚（40×50/40 = 50）
    let ok = a.mean_of(K_VOLUNTARY) == Ok(50);
    s.add("56164-J2-均值锚", ok, if ok { "" } else { "均值漂移" });
    // J2-2 migrate 均值（10×500/10 = 500）
    let ok = a.mean_of(K_MIGRATE) == Ok(500);
    s.add("56164-J2-migrate均值", ok, if ok { "" } else { "均值漂移" });
    // J2-3 均值哨兵核（零计数显式 Err）
    let a2 = CtxSwitchAggregator::new();
    let ok = a2.mean_of(K_PREEMPT) == Err(CS_ZERO_MEAN);
    s.add("56164-J2-零计数拒", ok, if ok { "" } else { "哨兵漂移" });
    // J2-4 mean_latency 底核（0/0 = 0 哨兵）
    let ok = mean_latency(0, 0) == 0 && mean_latency(100, 3) == 33;
    s.add("56164-J2-底核算", ok, if ok { "" } else { "底核漂移" });
    // J2-5 总延迟核对（voluntary 2000）
    let ok = a.agg(K_VOLUNTARY).map(|x| x.total_lat == 2000).unwrap_or(false);
    s.add("56164-J2-总延迟核", ok, if ok { "" } else { "总延迟漂移" });
    // J3-1 接管印
    let mut a = standard_agg();
    let ok = a.seal() && a.sealed();
    s.add("56164-J3-接管印", ok, if ok { "" } else { "接管印失败" });
    // J3-2 印后只读
    let ok = a.record(K_VOLUNTARY, 1) == Err(CS_SEALED_WRITE);
    s.add("56164-J3-印后只读", ok, if ok { "" } else { "只读闸失效" });
    // J3-3 印文渲染
    let ok = a.render_agg().contains("CTXSW-SEAL");
    s.add("56164-J3-印文渲染", ok, if ok { "" } else { "印文漂移" });
    // J3-4 聚合指纹确定
    let ok = standard_agg().agg_fp() == standard_agg().agg_fp();
    s.add("56164-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56164-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56164_all_checks_green() {
        let set = run_unx56164_checks();
        assert!(set.all_passed(), "UNX-F56164 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56164_mean_stable() {
        assert_eq!(mean_latency(100, 3), 33);
        assert_eq!(mean_latency(0, 0), 0);
    }
}
