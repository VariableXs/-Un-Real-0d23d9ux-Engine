//! UNX-F56189 启动重放复测执行件（0x617A0 段）
//!
//! 锚点：UNX-F56189-J1 重放复测账（同一启动样本重放 N 次——轮次 ×
//! 总时长 × 三段抽查逐轮落账）；J2 复测语义（轮数达标 + 全轮序合法
//! + 方差在阈三性质）；J3 复测指纹（轮账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F2-B40（启动域·重放复测面）。
//!
//! 结构：重放轮账 + 复测执行核（轮序闸 + 时长闸 + 方差闸）+ 复测指
//! 纹 + 注入核（零时长轮/时长倒挂/未知段抽查/轮数不足/印后追加必
//! 抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x617A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x617A0;
pub const RP_ZERO_ROUND: u32 = SEG_BASE + 0x01; // 零时长轮
pub const RP_INVERT: u32 = SEG_BASE + 0x02; // 时长倒挂（负值包装）
pub const RP_UNK_PROBE: u32 = SEG_BASE + 0x03; // 未知段抽查
pub const RP_SHORT_RUN: u32 = SEG_BASE + 0x04; // 轮数不足
pub const RP_FROZEN: u32 = SEG_BASE + 0x05; // 印后追加

/// 单轮重放账。
#[derive(Clone)]
pub struct ReplayRound {
    pub round: usize,
    pub total_ms: u64,
    /// 三段抽查：boot / kernel / interact 时长。
    pub probe: [u64; 3],
}

impl ReplayRound {
    /// 轮合法性：总时长 > 0 且三段抽查均 ≤ 总时长。
    pub fn round_ok(&self) -> bool {
        self.total_ms > 0 && self.probe.iter().all(|&p| p <= self.total_ms)
    }

    fn render_line(&self) -> String {
        format!(
            "R{}|{}|p={},{},{}",
            self.round,
            self.total_ms,
            self.probe[0],
            self.probe[1],
            self.probe[2]
        )
    }
}

/// 重放复测执行器。
pub struct ReplayTester {
    rounds: Vec<ReplayRound>,
    min_rounds: usize,
    var_threshold: u64,
    sealed: bool,
}

impl ReplayTester {
    /// 新执行器：min_rounds 最少轮数，var_threshold 方差阈（毫秒）。
    pub fn new(min_rounds: usize, var_threshold: u64) -> Self {
        ReplayTester {
            rounds: Vec::new(),
            min_rounds,
            var_threshold,
            sealed: false,
        }
    }

    /// 记录一轮：零时长闸 + 倒挂闸 + 印后拒。
    pub fn push_round(&mut self, total_ms: u64, probe: [u64; 3]) -> Result<(), u32> {
        if self.sealed {
            return Err(RP_FROZEN);
        }
        if total_ms == 0 {
            return Err(RP_ZERO_ROUND);
        }
        if probe.iter().any(|&p| p > total_ms) {
            return Err(RP_INVERT);
        }
        self.rounds.push(ReplayRound {
            round: self.rounds.len(),
            total_ms,
            probe,
        });
        Ok(())
    }

    pub fn round(&self, idx: usize) -> Option<&ReplayRound> {
        self.rounds.get(idx)
    }

    pub fn rounds_done(&self) -> usize {
        self.rounds.len()
    }

    /// 轮数达标核。
    pub fn rounds_sufficient(&self) -> bool {
        self.rounds.len() >= self.min_rounds
    }

    /// 总时长均值。
    pub fn mean_ms(&self) -> u64 {
        if self.rounds.is_empty() {
            return 0;
        }
        let sum: u64 = self.rounds.iter().map(|r| r.total_ms).sum();
        sum / self.rounds.len() as u64
    }

    /// 极差方差：max - min（守得住的方差口径）。
    pub fn range_ms(&self) -> u64 {
        if self.rounds.is_empty() {
            return 0;
        }
        let mut lo = u64::MAX;
        let mut hi = 0u64;
        for r in &self.rounds {
            lo = lo.min(r.total_ms);
            hi = hi.max(r.total_ms);
        }
        hi - lo
    }

    /// 方差在阈核。
    pub fn var_within(&self) -> bool {
        self.range_ms() <= self.var_threshold
    }

    /// 复测合格总核：轮数达标 + 全轮合法 + 方差在阈。
    pub fn verdict_ok(&self) -> bool {
        self.rounds_sufficient()
            && self.rounds.iter().all(|r| r.round_ok())
            && self.var_within()
    }

    /// 复测指纹。
    pub fn replay_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for r in &self.rounds {
            h = h.wrapping_mul(0x0100_0193) ^ (r.total_ms as u32);
            for p in r.probe {
                h = h.wrapping_mul(0x0100_0193) ^ (p as u32);
            }
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封测印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || !self.verdict_ok() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_replay(&self) -> String {
        let mut out = String::new();
        for r in &self.rounds {
            out.push_str(&r.render_line());
            out.push('\n');
        }
        out.push_str(&format!("REPLAY|mean={}|range={}\n", self.mean_ms(), self.range_ms()));
        if self.sealed {
            out.push_str("REPLAY-SEAL\n");
        }
        out
    }
}

/// 标准夹具：5 轮重放（总时长 5980~6020，方差阈 50）。
fn standard_tester() -> ReplayTester {
    let mut t = ReplayTester::new(5, 50);
    let rounds: [(u64, [u64; 3]); 5] = [
        (6000, [800, 1800, 800]),
        (5985, [795, 1790, 790]),
        (6010, [805, 1810, 805]),
        (5995, [798, 1795, 798]),
        (6020, [802, 1805, 802]),
    ];
    for (total, probe) in rounds {
        let _ = t.push_round(total, probe);
    }
    t
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：零时长轮
    let mut t = ReplayTester::new(3, 100);
    if t.push_round(0, [0, 0, 0]) == Err(RP_ZERO_ROUND) {
        caught += 1;
    }
    // 注 2：抽查超总（倒挂包装）
    let mut t = ReplayTester::new(3, 100);
    if t.push_round(1000, [800, 1800, 800]) == Err(RP_INVERT) {
        caught += 1;
    }
    // 注 3：轮数不足封测拒
    let mut t = ReplayTester::new(5, 50);
    let _ = t.push_round(6000, [800, 1800, 800]);
    if !t.seal() && !t.sealed() {
        caught += 1;
    }
    // 注 4：方差超阈封测拒
    let mut t = ReplayTester::new(3, 10);
    let _ = t.push_round(5000, [800, 1800, 800]);
    let _ = t.push_round(6000, [800, 1800, 800]);
    let _ = t.push_round(5500, [800, 1800, 800]);
    if !t.seal() && !t.sealed() {
        caught += 1;
    }
    // 注 5：印后追加
    let mut t = standard_tester();
    let _ = t.seal();
    if t.push_round(6000, [800, 1800, 800]) == Err(RP_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 重放复测账；J2 复测语义；J3 复测指纹。
pub fn run_unx56189_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56189");
    // J1-1 五轮全录
    let t = standard_tester();
    let ok = t.rounds_done() == 5 && (0..5).all(|i| t.round(i).is_some());
    s.add("56189-J1-五轮全录", ok, if ok { "" } else { "轮账不全" });
    // J1-2 渲染导出齐
    let mut t2 = standard_tester();
    let _ = t2.seal();
    let text = t2.render_replay();
    let ok = text.contains("R0|6000|") && text.contains("REPLAY|mean=") && text.contains("REPLAY-SEAL");
    s.add("56189-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 轮数达标
    let ok = t.rounds_sufficient();
    s.add("56189-J2-轮数达标", ok, if ok { "" } else { "轮数不足" });
    // J2-2 全轮合法
    let ok = (0..t.rounds_done()).all(|i| t.round(i).map(|r| r.round_ok()).unwrap_or(false));
    s.add("56189-J2-全轮合法", ok, if ok { "" } else { "轮合法性漂移" });
    // J2-3 均值核（5980+585+6010+5995+6020 /5 = 6002）
    let ok = t.mean_ms() == 6_002;
    s.add("56189-J2-均值核", ok, if ok { "" } else { "均值漂移" });
    // J2-4 极差核（6020-5985 = 35 ≤ 50）
    let ok = t.range_ms() == 35;
    s.add("56189-J2-极差核", ok, if ok { "" } else { "极差漂移" });
    // J2-5 方差在阈
    let ok = t.var_within();
    s.add("56189-J2-方差在阈", ok, if ok { "" } else { "方差超阈" });
    // J2-6 复测合格总核
    let ok = t.verdict_ok();
    s.add("56189-J2-复测合格", ok, if ok { "" } else { "总核失效" });
    // J3-1 复测指纹确定
    let ok = standard_tester().replay_fp() == standard_tester().replay_fp();
    s.add("56189-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 轮变签变
    let mut t3 = ReplayTester::new(3, 1000);
    let _ = t3.push_round(6000, [800, 1800, 800]);
    let fp0 = t3.replay_fp();
    let _ = t3.push_round(6050, [800, 1800, 800]);
    let ok = t3.replay_fp() != fp0;
    s.add("56189-J3-轮变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-4 封测印
    let mut t4 = standard_tester();
    let ok = t4.seal() && t4.sealed();
    s.add("56189-J3-封测印", ok, if ok { "" } else { "封印失败" });
    // J3-5 印后拒录
    let ok = t4.push_round(6000, [800, 1800, 800]) == Err(RP_FROZEN);
    s.add("56189-J3-印后拒录", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56189-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56189_all_checks_green() {
        let set = run_unx56189_checks();
        assert!(set.all_passed(), "UNX-F56189 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56189_replay_stable() {
        assert!(standard_tester().verdict_ok());
    }
}
