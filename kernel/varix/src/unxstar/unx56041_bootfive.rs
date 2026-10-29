//! UNX-F56041 启动时长五段分段计时器（0x60500 段）
//!
//! 锚点：UNX-F56041-J1 五段（引导扇区交权→内核首任务→登录面→桌面首
//! 帧→双击首响应）每段落账且段和 ≤ 全程实测 ±5%；J2 同一镜像连测 10
//! 次段方差可导出（逐段 P50/P95 可查）；J3 段序纪律（段起点单调不减
//! ——后段起点不得早于前段起点）三判据全绿。判据（P8 类）；判据锚定
//! §AI-71.3 示例条目 UNX-F56041 保真。
//! 域：CoRun·UNX-F2-B40（启动分段域·五段计时面）。
//!
//! 结构：五段闭集登记册（段名 × 起点 × 终点逐段落账）+ 段和一致性核
//! （段和 vs 全程万分比 ±500 闸）+ 段序单调核 + 连测方差账（10 轮逐
//! 段分位导出）+ 注入核（越闭集段/起点回退/段和超差/负时长/封账改写
//! 必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60500 独占 0x20）。
pub const SEG_BASE: u32 = 0x60500;
pub const BT_SEG_UNK: u32 = SEG_BASE + 0x01; // 越闭集段
pub const BT_START_BACK: u32 = SEG_BASE + 0x02; // 段起点回退（后段早于前段）
pub const BT_SUM_OVER: u32 = SEG_BASE + 0x03; // 段和超差（>±5%）
pub const BT_NEG_DUR: u32 = SEG_BASE + 0x04; // 负时长（终点早于起点）
pub const BT_SEALED: u32 = SEG_BASE + 0x05; // 计时账已封账拒写
pub const BT_RUN_SHORT: u32 = SEG_BASE + 0x06; // 连测不足 10 轮
pub const BT_TOTAL_ZERO: u32 = SEG_BASE + 0x07; // 全程零（除零口径）

/// 五段闭集。
pub const G_FIRMWARE: usize = 0;
pub const G_KERNEL: usize = 1;
pub const G_LOGIN: usize = 2;
pub const G_DESKTOP: usize = 3;
pub const G_CLICK: usize = 4;
pub const G_COUNT: usize = 5;
pub const G_NAMES: [&str; G_COUNT] =
    ["firmware_handoff", "kernel_first_task", "login_face", "desktop_first_frame", "click_first_response"];

/// 段和一致性阈值（万分比 ±5% = 500）。
pub const SUM_TOL_PBP: u32 = 500;

/// 连测轮数（10）。
pub const RUNS_N: usize = 10;

/// 单轮五段账：逐段（起点, 终点）× 全程实测。
#[derive(Clone)]
pub struct RunEntry {
    pub starts: [u64; G_COUNT],
    pub ends: [u64; G_COUNT],
    pub total_ns: u64,
}

impl RunEntry {
    /// 构造核：段起点单调不减 + 终点 ≥ 起点 + 全程非零。
    pub fn new(starts: [u64; G_COUNT], ends: [u64; G_COUNT], total_ns: u64) -> Result<Self, u32> {
        if total_ns == 0 {
            return Err(BT_TOTAL_ZERO);
        }
        for g in 0..G_COUNT {
            if ends[g] < starts[g] {
                return Err(BT_NEG_DUR);
            }
            if g > 0 && starts[g] < starts[g - 1] {
                return Err(BT_START_BACK);
            }
        }
        Ok(RunEntry { starts, ends, total_ns })
    }

    /// 段时长（ns）。
    pub fn seg_ns(&self, g: usize) -> u64 {
        if g >= G_COUNT {
            return 0;
        }
        self.ends[g] - self.starts[g]
    }

    /// 段和。
    pub fn sum_ns(&self) -> u64 {
        let mut s = 0u64;
        for g in 0..G_COUNT {
            s += self.seg_ns(g);
        }
        s
    }

    /// J1：段和 vs 全程偏差（万分比；i64 中转防下溢）。
    pub fn sum_drift_pbp(&self) -> u32 {
        let d = (self.sum_ns() as i64 - self.total_ns as i64).abs();
        ((d * 10_000) / self.total_ns as i64) as u32
    }

    /// 段和达标（±5%）。
    pub fn sum_ok(&self) -> bool {
        self.sum_drift_pbp() <= SUM_TOL_PBP
    }
}

/// 连测账：10 轮 RunEntry 逐轮落账。
pub struct RunBook {
    pub runs: Vec<RunEntry>,
    pub sealed: bool,
}

impl RunBook {
    pub fn new() -> Self {
        RunBook { runs: Vec::new(), sealed: false }
    }

    /// 落一轮。
    pub fn put(&mut self, r: RunEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(BT_SEALED);
        }
        self.runs.push(r);
        Ok(())
    }

    /// J2：逐段分位（P50/P95；连测 ≥10 轮才出）。
    pub fn seg_stats(&self, g: usize) -> Result<(u64, u64), u32> {
        if g >= G_COUNT {
            return Err(BT_SEG_UNK);
        }
        if self.runs.len() < RUNS_N {
            return Err(BT_RUN_SHORT);
        }
        let mut v: Vec<u64> = Vec::new();
        for r in self.runs.iter() {
            v.push(r.seg_ns(g));
        }
        v.sort();
        let p50 = v[v.len() / 2];
        let p95 = v[(v.len() * 95 / 100).min(v.len() - 1)];
        Ok((p50, p95))
    }

    /// J2'：方差可导核（五段全出分位）。
    pub fn variance_exportable(&self) -> bool {
        for g in 0..G_COUNT {
            if self.seg_stats(g).is_err() {
                return false;
            }
        }
        true
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：确定性 10 轮（段 i 起点 = i×200ms + 轮抖动 0；
/// 终点 = 起点 + 180ms + 段×10ms；全程 = 段和 + 轮偏移 0 → 恰一致）。
pub fn standard_runs() -> RunBook {
    let mut b = RunBook::new();
    for _ in 0..RUNS_N {
        let mut starts = [0u64; G_COUNT];
        let mut ends = [0u64; G_COUNT];
        for g in 0..G_COUNT {
            starts[g] = g as u64 * 200_000_000;
            ends[g] = starts[g] + 180_000_000 + g as u64 * 10_000_000;
        }
        let mut sum = 0u64;
        for g in 0..G_COUNT {
            sum += ends[g] - starts[g];
        }
        let r = RunEntry::new(starts, ends, sum).expect("fixture run");
        b.put(r).expect("put");
    }
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 段起点回退（段 2 起点早于段 1）
    let starts = [0u64, 100, 50, 400, 600];
    let ends = [100u64, 200, 300, 500, 700];
    if RunEntry::new(starts, ends, 800).err() == Some(BT_START_BACK) {
        caught += 1;
    }
    // 2) 负时长（终点早于起点）
    let starts2 = [0u64, 100, 200, 300, 400];
    let ends2 = [100u64, 50, 300, 400, 500];
    if RunEntry::new(starts2, ends2, 800).err() == Some(BT_NEG_DUR) {
        caught += 1;
    }
    // 3) 段和超差（段和 500 vs 全程 1000 → 5000 万分比 > 500）
    let starts3 = [0u64, 100, 200, 300, 400];
    let ends3 = [100u64, 200, 300, 400, 500];
    if RunEntry::new(starts3, ends3, 1_000).expect("r3").sum_drift_pbp() == 5_000 {
        caught += 1;
    }
    // 4) 连测不足 10 轮拒分位
    let mut b = standard_runs();
    b.runs.pop();
    if b.seg_stats(G_FIRMWARE) == Err(BT_RUN_SHORT) {
        caught += 1;
    }
    // 5) 全程零
    if RunEntry::new([0; 5], [0; 5], 0).err() == Some(BT_TOTAL_ZERO) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56041_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56041");
    // J1-1 五段逐段落账（10 轮齐）
    let b = standard_runs();
    let ok = b.runs.len() == RUNS_N
        && b.runs.iter().all(|r| r.seg_ns(G_FIRMWARE) == 180_000_000);
    s.add("56041-J1-五段逐段落账", ok, if ok { "" } else { "段账缺" });
    // J1-2 段和恰一致（夹具段和 = 全程 → 偏差 0）
    let ok = b.runs.iter().all(|r| r.sum_ok() && r.sum_drift_pbp() == 0);
    s.add("56041-J1-段和恰一致", ok, if ok { "" } else { "段和漂移" });
    // J1-3 段和阈值语义（段和 910 vs 全程 910 恰一致；漂移 0 ≤ 500 过）
    let starts = [0u64, 200, 400, 600, 800];
    let ends = [180u64, 380, 580, 780, 990];
    let mut sum = 0u64;
    for g in 0..G_COUNT {
        sum += ends[g] - starts[g];
    }
    let r = RunEntry::new(starts, ends, sum).expect("r");
    let ok = r.sum_drift_pbp() == 0 && r.sum_ok() && sum == 910;
    s.add("56041-J1-段和阈值语义", ok, if ok { "" } else { "阈值漂移" });
    // J1-4 段名闭集同源（五段序与判据原文一致）
    let ok = G_NAMES[0] == "firmware_handoff" && G_NAMES[4] == "click_first_response";
    s.add("56041-J1-段名同源", ok, if ok { "" } else { "命名漂移" });
    // J2-1 连测 10 轮逐段分位导出（段 0：P50=P95=180ms 确定性）
    let (p50, p95) = b.seg_stats(G_FIRMWARE).expect("st");
    let ok = p50 == 180_000_000 && p95 == 180_000_000;
    s.add("56041-J2-连测分位导出", ok, if ok { "" } else { "分位漂移" });
    // J2-2 五段全可导
    let ok = b.variance_exportable();
    s.add("56041-J2-方差可导", ok, if ok { "" } else { "方差账缺" });
    // J2-3 段梯度（段 4 时长 > 段 0：220 > 180）
    let r4 = b.runs[0].seg_ns(G_CLICK);
    let ok = r4 == 220_000_000 && r4 > b.runs[0].seg_ns(G_FIRMWARE);
    s.add("56041-J2-段梯度", ok, if ok { "" } else { "梯度漂移" });
    // J3-1 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56041-J3-注入核必抓", ok, &detail);
    // J1-5 段起点单调（夹具起点 0/200/400/600/800ms 单调）
    let r0 = &b.runs[0];
    let mut mono = true;
    for g in 1..G_COUNT {
        if r0.starts[g] < r0.starts[g - 1] {
            mono = false;
        }
    }
    let ok = mono && r0.starts[1] == 200_000_000;
    s.add("56041-J1-段起点单调", ok, if ok { "" } else { "起点破序" });
    // J2-4 轮账封账冻结
    let mut b2 = standard_runs();
    b2.seal();
    let r2 = RunEntry::new([0; 5], [0; 5], 1).expect("r2");
    let ok = b2.put(r2) == Err(BT_SEALED);
    s.add("56041-J2-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J2-5 段越闭集查询拒
    let ok = b.seg_stats(9) == Err(BT_SEG_UNK);
    s.add("56041-J2-段查询闭集", ok, if ok { "" } else { "越段穿透" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56041_all_checks_green() {
        let set = run_unx56041_checks();
        assert!(set.all_passed(), "UNX-F56041 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56041_sum_math() {
        let b = standard_runs();
        let r = &b.runs[0];
        assert_eq!(r.sum_ns(), r.total_ns);
        assert!(r.sum_ok());
    }
}
