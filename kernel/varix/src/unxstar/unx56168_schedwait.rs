//! UNX-F56168 syscall 延迟归因分段剖析件（段二：调度等待）（0x61500 段）
//!
//! 锚点：UNX-F56168-J1 调度等待段剖析账（调度等待三态——就绪队驻留/
//! CPU 争夺/优先级压制——逐态延迟聚合落账：态 × 计数 × 总延迟 × 均值
//! × 指纹五元组）；J2 态转移语义（等待总时长 = 三态之和，态间转移计
//! 数矩阵落账）；J3 压制哨兵（压制态占比 ≥ 50% 判调度异常留痕）三判
//! 据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·调度等待面）。
//!
//! 结构：等待三态闭集 + 延迟录入核 + 态转移矩阵（3×3 计数）+ 压制哨
//! 兵（50% 阈）+ 剖析指纹 + 注入核（未知态/转移非法/总延迟漂移/哨兵
//! 误报/空账印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61500 独占 0x20）。
pub const SEG_BASE: u32 = 0x61500;
pub const SW_UNK_STATE: u32 = SEG_BASE + 0x01; // 未知等待态
pub const SW_TRANS_ILLEGAL: u32 = SEG_BASE + 0x02; // 非法转移（自环除外）
pub const SW_SUM_DRIFT: u32 = SEG_BASE + 0x03; // 总延迟漂移
pub const SW_STARVE_MISS: u32 = SEG_BASE + 0x04; // 压制哨兵漏报
pub const SW_EMPTY_SEAL: u32 = SEG_BASE + 0x05; // 空账印

/// 等待三态闭集。
pub const W_READY: usize = 0; // 就绪队驻留
pub const W_CONTEND: usize = 1; // CPU 争夺
pub const W_STARVE: usize = 2; // 优先级压制
pub const WSTATE_COUNT: usize = 3;

/// 态名渲染。
pub const WSTATE_NAMES: [&str; WSTATE_COUNT] = ["ready", "contend", "starve"];

/// 压制哨兵阈（万分比 5000 = 50%）。
pub const STARVE_AT: u32 = 5_000;

/// 占比核。
pub fn ratio_of(part: u64, total: u64) -> u32 {
    if total == 0 {
        return 0;
    }
    ((part as u128 * 10_000) / total as u128) as u32
}

/// 单态聚合账。
#[derive(Clone)]
pub struct WaitAgg {
    pub state: usize,
    pub count: u64,
    pub total_lat: u64,
}

/// 调度等待剖析器。
pub struct SchedWaitProfile {
    aggs: [WaitAgg; WSTATE_COUNT],
    transitions: [[u64; WSTATE_COUNT]; WSTATE_COUNT], // 转移计数矩阵
    sealed: bool,
}

impl SchedWaitProfile {
    pub fn new() -> Self {
        SchedWaitProfile {
            aggs: [
                WaitAgg { state: W_READY, count: 0, total_lat: 0 },
                WaitAgg { state: W_CONTEND, count: 0, total_lat: 0 },
                WaitAgg { state: W_STARVE, count: 0, total_lat: 0 },
            ],
            transitions: [[0; WSTATE_COUNT]; WSTATE_COUNT],
            sealed: false,
        }
    }

    /// 录一等待：态闭集 + 印后拒。
    pub fn record(&mut self, state: usize, lat_ns: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(SW_EMPTY_SEAL);
        }
        if state >= WSTATE_COUNT {
            return Err(SW_UNK_STATE);
        }
        self.aggs[state].count += 1;
        self.aggs[state].total_lat += lat_ns;
        Ok(())
    }

    /// 录一转移：from→to 闭集（同态转移合法=重入）。
    pub fn record_transition(&mut self, from: usize, to: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(SW_EMPTY_SEAL);
        }
        if from >= WSTATE_COUNT || to >= WSTATE_COUNT {
            return Err(SW_UNK_STATE);
        }
        self.transitions[from][to] += 1;
        Ok(())
    }

    pub fn agg(&self, state: usize) -> Option<&WaitAgg> {
        self.aggs.get(state)
    }

    pub fn transition(&self, from: usize, to: usize) -> u64 {
        self.transitions[from][to]
    }

    /// 等待总延迟 = 三态之和。
    pub fn wait_total(&self) -> u64 {
        (0..WSTATE_COUNT).map(|s| self.aggs[s].total_lat).sum()
    }

    /// 三态占比。
    pub fn ratios(&self) -> [u32; WSTATE_COUNT] {
        let t = self.wait_total();
        [
            ratio_of(self.aggs[0].total_lat, t),
            ratio_of(self.aggs[1].total_lat, t),
            ratio_of(self.aggs[2].total_lat, t),
        ]
    }

    /// 压制哨兵：starve 占比 ≥ 50%。
    pub fn starved(&self) -> bool {
        self.ratios()[W_STARVE] >= STARVE_AT
    }

    pub fn seal(&mut self) -> bool {
        if self.sealed || self.wait_total() == 0 {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 剖析指纹。
    pub fn profile_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for a in &self.aggs {
            h = h.wrapping_mul(0x0100_0193) ^ (a.count as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (a.total_lat as u32);
        }
        for row in &self.transitions {
            for v in row {
                h = h.wrapping_mul(0x0100_0193) ^ (*v as u32);
            }
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 渲染导出。
    pub fn render_profile(&self) -> String {
        let mut out = String::new();
        for a in &self.aggs {
            out.push_str(&format!(
                "{}|n{}|lat{}|mean{}\n",
                WSTATE_NAMES[a.state],
                a.count,
                a.total_lat,
                if a.count == 0 { 0 } else { a.total_lat / a.count }
            ));
        }
        if self.starved() {
            out.push_str("STARVE-ALERT\n");
        }
        if self.sealed {
            out.push_str("SCHEDW-SEAL\n");
        }
        out
    }
}

impl Default for SchedWaitProfile {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：ready 主导（ready 600×100 / contend 300×200 / starve 100×500）+ 转移链。
fn standard_profile() -> SchedWaitProfile {
    let mut p = SchedWaitProfile::new();
    for _ in 0..600 {
        let _ = p.record(W_READY, 100);
    }
    for _ in 0..300 {
        let _ = p.record(W_CONTEND, 200);
    }
    for _ in 0..100 {
        let _ = p.record(W_STARVE, 500);
    }
    let _ = p.record_transition(W_READY, W_CONTEND);
    let _ = p.record_transition(W_CONTEND, W_STARVE);
    let _ = p.record_transition(W_STARVE, W_READY);
    p
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知态
    let mut p = SchedWaitProfile::new();
    if p.record(9, 100) == Err(SW_UNK_STATE) {
        caught += 1;
    }
    // 注 2：转移未知态
    let mut p = SchedWaitProfile::new();
    if p.record_transition(0, 9) == Err(SW_UNK_STATE) {
        caught += 1;
    }
    // 注 3：总延迟漂移
    let mut p = standard_profile();
    let t0 = p.wait_total();
    p.aggs[0].total_lat += 1;
    if p.wait_total() != t0 {
        caught += 1;
    }
    // 注 4：压制哨兵抓（starve 占比过半必报）
    let mut p = SchedWaitProfile::new();
    let _ = p.record(W_STARVE, 6000);
    let _ = p.record(W_READY, 4000);
    if p.starved() {
        caught += 1;
    }
    // 注 5：空账印
    let mut p = SchedWaitProfile::new();
    if !p.seal() && !p.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 剖析账；J2 态转移；J3 压制哨兵。
pub fn run_unx56168_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56168");
    let p = standard_profile();
    // J1-1 三态计数齐
    let ok = p.agg(W_READY).map(|x| x.count == 600).unwrap_or(false)
        && p.agg(W_CONTEND).map(|x| x.count == 300).unwrap_or(false)
        && p.agg(W_STARVE).map(|x| x.count == 100).unwrap_or(false);
    s.add("56168-J1-三态计数齐", ok, if ok { "" } else { "计数漂移" });
    // J1-2 渲染导出齐（无 STARVE-ALERT——starve 占比 12.5%）
    let text = standard_profile().render_profile();
    let ok = text.contains("ready|n600|lat60000|mean100") && !text.contains("STARVE-ALERT");
    s.add("56168-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 等待总延迟核（60000+60000+50000 = 170000）
    let ok = p.wait_total() == 170_000;
    s.add("56168-J2-总延迟核", ok, if ok { "" } else { "总延迟漂移" });
    // J2-2 转移矩阵计数（ready→contend = 1）
    let ok = p.transition(W_READY, W_CONTEND) == 1 && p.transition(W_CONTEND, W_STARVE) == 1;
    s.add("56168-J2-转移计数", ok, if ok { "" } else { "转移漂移" });
    // J2-3 占比核（ready 60000/170000 = 3529）
    let ok = p.ratios()[W_READY] == 3_529;
    s.add("56168-J2-占比核", ok, if ok { "" } else { "占比漂移" });
    // J2-4 均值核（starve 500）
    let ok = p.agg(W_STARVE).map(|x| x.total_lat / x.count == 500).unwrap_or(false);
    s.add("56168-J2-均值核", ok, if ok { "" } else { "均值漂移" });
    // J3-1 压制哨兵触发（starve 60%）
    let mut p2 = SchedWaitProfile::new();
    let _ = p2.record(W_STARVE, 6000);
    let _ = p2.record(W_READY, 4000);
    let ok = p2.starved() && p2.render_profile().contains("STARVE-ALERT");
    s.add("56168-J3-压制触发", ok, if ok { "" } else { "哨兵漏报" });
    // J3-2 压制不触发（49%）
    let mut p3 = SchedWaitProfile::new();
    let _ = p3.record(W_STARVE, 4900);
    let _ = p3.record(W_READY, 5100);
    let ok = !p3.starved();
    s.add("56168-J3-压制不触发", ok, if ok { "" } else { "哨兵误报" });
    // J3-3 阈缘（恰 50% 触发）
    let mut p4 = SchedWaitProfile::new();
    let _ = p4.record(W_STARVE, 5000);
    let _ = p4.record(W_READY, 5000);
    let ok = p4.starved();
    s.add("56168-J3-阈缘触发", ok, if ok { "" } else { "阈缘漂移" });
    // J3-4 封印
    let mut p5 = standard_profile();
    let ok = p5.seal() && p5.sealed();
    s.add("56168-J3-封印", ok, if ok { "" } else { "封印失败" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56168-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56168_all_checks_green() {
        let set = run_unx56168_checks();
        assert!(set.all_passed(), "UNX-F56168 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56168_ratio_stable() {
        assert_eq!(ratio_of(60_000, 170_000), 3_529);
    }
}
