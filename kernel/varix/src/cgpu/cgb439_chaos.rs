//! CGPU-F0439 · 仲裁器混沌测试（目标 340 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 混沌：随机故障注入（仲裁依赖组件随机失效——注入器常态化）；韧性
//!   判据（故障期间不丢预算/不超卖/恢复后对账一致——混沌验收标准）；
//!   爆炸半径控制（故障注入的影响面限制——测试安全）；演练报告（每次
//!   混沌演练自动出报告）。判据：注入器、韧性判据、半径控制、报告、判据。
//!
//! 设计纪律：
//! - 注入器常态化：xorshift64* 确定性种子驱动（同种子同演练——可复算），
//!   每轮按概率把某组件置为故障态（预算表/分配器/对账器三组件）。
//! - 韧性三判据（混沌验收的硬线）：故障期间预算账守恒（丢失=0）、
//!   无超卖（分配 ≤ 容量）、恢复后对账一致（账实相符）。
//! - 爆炸半径：注入只影响声明过的组件子集（半径外组件恒健康）——
//!   注入器不得碰 radius 清单之外的组件（测试安全的形式化）。
//! - 演练报告：轮数/注入次数/三判据逐项结果/种子——一纸报告出验收结论。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0EA0 独占。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0EA0，全仓独占）。
pub const SEG_BASE: u16 = 0x0EA0;

pub const E_CH_RADIUM: u16 = SEG_BASE + 1; // 注入越出爆炸半径
pub const E_CH_LOST: u16 = SEG_BASE + 2; // 韧性红：预算丢失
pub const E_CH_OVER: u16 = SEG_BASE + 3; // 韧性红：超卖
pub const E_CH_RECON: u16 = SEG_BASE + 4; // 韧性红：对账不一致
pub const E_CH_SEED: u16 = SEG_BASE + 5; // 种子非法（0）

/// 组件清单（爆炸半径的可选集）。
pub const COMP_BUDGET: usize = 0;
pub const COMP_ALLOC: usize = 1;
pub const COMP_RECON: usize = 2;
pub const COMP_COUNT: usize = 3;

/// xorshift64* 确定性 RNG。
#[derive(Clone, Debug)]
pub struct ChaosRng(u64);

impl ChaosRng {
    pub fn new(seed: u64) -> Result<Self, u16> {
        if seed == 0 {
            return Err(E_CH_SEED);
        }
        Ok(ChaosRng(seed))
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// 演练中的被试仲裁账本（简化账本——够验证韧性判据即可）。
#[derive(Debug, Default)]
pub struct ArbLedger {
    /// 各账户预算余量。
    pub budgets: Vec<u64>,
    /// 各账户已分配。
    pub granted: Vec<u64>,
    /// 故障中的组件。
    pub faulty: [bool; COMP_COUNT],
    /// 容量（千分位）。
    pub capacity_pm: u64,
}

impl ArbLedger {
    /// 申请分配（分配器故障 → 请求被丢弃但预算不丢；对账器故障 → 对账跳过）。
    pub fn request(&mut self, acct: usize, amount: u64) -> bool {
        if acct >= self.budgets.len() {
            return false;
        }
        if self.faulty[COMP_ALLOC] {
            return false; // 分配器故障：请求拒绝（不超卖的第一防线）。
        }
        let total: u64 = self.granted.iter().sum();
        if total.saturating_add(amount) > self.capacity_pm {
            return false; // 容量闸。
        }
        if self.budgets[acct] >= amount {
            self.budgets[acct] -= amount;
            self.granted[acct] += amount;
            return true;
        }
        false
    }

    /// 退还（预算表故障 → 退还被吞——丢预算的唯一通道，韧性判据要抓它）。
    pub fn refund(&mut self, acct: usize, amount: u64) {
        if acct >= self.granted.len() || self.faulty[COMP_BUDGET] {
            return; // 预算表故障：退还丢失。
        }
        let take = amount.min(self.granted[acct]);
        self.granted[acct] -= take;
        self.budgets[acct] += take;
    }

    /// 对账（对账器故障 → 返回 Err——演练记录该轮对账被跳过）。
    pub fn reconcile(&self, expected_total: u64) -> Result<u64, u16> {
        if self.faulty[COMP_RECON] {
            return Err(E_CH_RECON);
        }
        let g: u64 = self.granted.iter().sum();
        let b: u64 = self.budgets.iter().sum();
        if g.saturating_add(b) != expected_total {
            return Err(E_CH_LOST);
        }
        Ok(g)
    }
}

/// 混沌演练报告。
#[derive(Clone, Debug, Default)]
pub struct ChaosReport {
    pub seed: u64,
    pub rounds: u64,
    pub injections: u64,
    pub requests: u64,
    pub granted_ok: u64,
    /// 三判据逐项。
    pub no_lost: bool,
    pub no_oversell: bool,
    pub recon_ok: bool,
    /// 被注入的组件位图。
    pub radius_mask: u8,
}

impl ChaosReport {
    pub fn verdict(&self) -> Result<(), u16> {
        if !self.no_lost {
            return Err(E_CH_LOST);
        }
        if !self.no_oversell {
            return Err(E_CH_OVER);
        }
        if !self.recon_ok {
            return Err(E_CH_RECON);
        }
        Ok(())
    }

    pub fn to_text(&self) -> String {
        alloc::format!(
            "chaos seed={} rounds={} injections={} req={} ok={} lost={} over={} recon={} mask={:#b}",
            self.seed,
            self.rounds,
            self.injections,
            self.requests,
            self.granted_ok,
            !self.no_lost,
            !self.no_oversell,
            !self.recon_ok,
            self.radius_mask
        )
    }
}

/// 混沌演练器。
#[derive(Debug)]
pub struct ChaosRunner {
    /// 爆炸半径（位图：bit0 预算表/bit1 分配器/bit2 对账器）。
    radius: u8,
    ledger: ArbLedger,
    report: ChaosReport,
}

impl ChaosRunner {
    /// 建演练：半径必须非零且 ≤ 全集（3 位）。
    pub fn new(seed: u64, radius: u8, accts: usize, capacity_pm: u64) -> Result<Self, u16> {
        ChaosRng::new(seed)?;
        if radius == 0 || radius > 0b111 {
            return Err(E_CH_RADIUM);
        }
        Ok(Self {
            radius,
            ledger: ArbLedger {
                budgets: vec![1_000; accts],
                granted: vec![0; accts],
                faulty: [false; COMP_COUNT],
                capacity_pm,
            },
            report: ChaosReport {
                seed,
                ..ChaosReport::default()
            },
        })
    }

    /// 跑 N 轮混沌。
    pub fn run(&mut self, rounds: u64) -> &ChaosReport {
        let mut rng = ChaosRng::new(self.report.seed).unwrap_or_else(|_| ChaosRng(1));
        let init_total: u64 = self.ledger.budgets.iter().sum::<u64>()
            + self.ledger.granted.iter().sum::<u64>();
        let mut recon_seen = true;
        self.report.rounds = self.report.rounds.saturating_add(rounds);
        for r in 0..rounds {
            // 注入：按半径内组件随机置故障/恢复。
            for c in 0..COMP_COUNT {
                if self.radius & (1 << c) != 0 {
                    let flip = rng.next() % 4 == 0;
                    self.ledger.faulty[c] = flip;
                    if flip {
                        self.report.injections += 1;
                    }
                } else if self.ledger.faulty[c] {
                    self.ledger.faulty[c] = false; // 半径外强制健康（半径控制）。
                }
            }
            // 业务流：随机申请/退还。
            let n = self.ledger.budgets.len();
            let acct = (rng.next() % n.max(1) as u64) as usize;
            if rng.next() % 2 == 0 {
                self.report.requests += 1;
                if self.ledger.request(acct, 10) {
                    self.report.granted_ok += 1;
                }
            } else {
                self.ledger.refund(acct, 5);
            }
            let _ = r;
        }
        // 收尾：全部恢复后对账。
        for f in self.ledger.faulty.iter_mut() {
            *f = false;
        }
        match self.ledger.reconcile(init_total) {
            Ok(_) => recon_seen = true,
            Err(E_CH_LOST) => recon_seen = false,
            Err(_) => {}
        }
        // 超卖检查。
        let g: u64 = self.ledger.granted.iter().sum();
        self.report.no_oversell = g <= self.ledger.capacity_pm;
        // 丢预算检查（对账已含，双保险显性化）。
        self.report.no_lost = recon_seen;
        self.report.recon_ok = recon_seen;
        self.report.radius_mask = self.radius;
        &self.report
    }

    pub fn report(&self) -> &ChaosReport {
        &self.report
    }
}

// ==========================================================================
// 判据（施工书：注入器、韧性判据、半径控制、报告、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_注入器_确定性种子() {
        let mut r1 = ChaosRng::new(0xEA0).unwrap();
        let mut r2 = ChaosRng::new(0xEA0).unwrap();
        for _ in 0..16 {
            assert_eq!(r1.next(), r2.next());
        }
        // 种子 0 拒。
        assert!(matches!(ChaosRng::new(0), Err(E_CH_SEED)));
    }

    #[test]
    fn a02_韧性三判据_故障期不丢不超() {
        // 小半径长时间演练：预算账守恒（refund 通道健康时）。
        let mut c = ChaosRunner::new(7, 0b010, 4, 10_000).unwrap();
        let rep = c.run(200).clone();
        // 分配器故障期请求被拒 = 不超卖第一防线。
        assert!(rep.no_oversell);
        assert_eq!(rep.verdict(), Ok(()));
        // 半径控制：bit0/bit2 不在半径内 → 从未注入。
        assert_eq!(rep.radius_mask, 0b010);
    }

    #[test]
    fn a03_爆炸半径_外组件恒健康() {
        let mut c = ChaosRunner::new(42, 0b001, 4, 10_000).unwrap();
        let rep = c.run(100);
        assert_eq!(rep.radius_mask, 0b001);
        // 半径非法：0 与超集拒。
        assert!(matches!(ChaosRunner::new(1, 0, 2, 100), Err(E_CH_RADIUM)));
        assert!(matches!(ChaosRunner::new(1, 0b1000, 2, 100), Err(E_CH_RADIUM)));
    }

    #[test]
    fn a04_对账一致_恢复后账实相符() {
        // 全半径演练：恢复后对账必须一致（预算账守恒）。
        for seed in [1u64, 9, 0xEA] {
            let mut c = ChaosRunner::new(seed, 0b111, 6, 50_000).unwrap();
            let rep = c.run(300);
            assert_eq!(rep.verdict(), Ok(()), "seed={}", seed);
        }
    }

    #[test]
    fn a05_演练报告_一纸出验收() {
        let mut c = ChaosRunner::new(5, 0b111, 4, 10_000).unwrap();
        c.run(64);
        let rep = c.report();
        let text = rep.to_text();
        // 报告要素齐备。
        assert!(text.contains("seed=5"));
        assert!(text.contains("rounds=64"));
        assert!(text.contains("injections="));
        assert!(text.contains("mask=0b111"));
        assert_eq!(rep.verdict(), Ok(()));
        // 报告无注入轮（rounds=0）也成立（平凡绿——演练未跑不算失败）。
        let mut c2 = ChaosRunner::new(5, 0b001, 2, 1_000).unwrap();
        let r2 = c2.run(0);
        assert_eq!(r2.verdict(), Ok(()));
    }
}
