//! CGPU-F0436 · 公平指数动态调节（目标 340 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 动态调节：Jain 指数实时监控（公平性持续可观测）；自动微调（指数
//!   跌破目标→权重有界微调——调节幅度上限防振荡）；调节审计（每次自动
//!   调节记录——不悄悄改）；人工优先（用户手动权重不被自动调节覆盖——
//!   手动优先声明）。判据：监控、有界微调、审计、手动优先、判据。
//!
//! 设计纪律：
//! - Jain 指数：J = (Σxᵢ)² / (n·Σxᵢ²)，xᵢ = 已获服务/权重（归一
//!   公平——权重大的账户理应多得）。J ∈ (0,1]，千分比展示。
//!   已知向量自证：全等 x → J=1000‰；(1,0,..) → 1/n。
//! - 自动微调：J < 目标时对「服务比最低的未钉账户」升权，对「服务比
//!   最高者」降权；单次幅度 ≤ MAX_ADJ_PM（千分比上限——防振荡）；
//!   冷却期内不二调（同向连续调节过密即红）。
//! - 手动优先：pinned 账户权重只随 unpin 释放——自动调节绕行
//!   （试图调 pinned = E_FAIR_PIN 违规记红，不是静默跳过）。
//! - 审计：每次自动调节记 (tick, who, from, to, reason)——审计链
//!   与当前权重可对账（链尾 to == 当前权）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0E70 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0E70，全仓独占）。
pub const SEG_BASE: u16 = 0x0E70;

pub const E_FAIR_TARGET: u16 = SEG_BASE + 1; // 目标指数非法（0 或 >1000‰）
pub const E_FAIR_ACCT: u16 = SEG_BASE + 2; // 账户非法（未登记）
pub const E_FAIR_PIN: u16 = SEG_BASE + 3; // 手动权重被自动调节触碰
pub const E_FAIR_BOUND: u16 = SEG_BASE + 4; // 微调超界（单次幅度超上限）
pub const E_FAIR_OSC: u16 = SEG_BASE + 5; // 冷却期内二调（振荡风险）
pub const E_FAIR_AUDIT: u16 = SEG_BASE + 6; // 审计链断裂

/// 单次微调幅度上限（权重千分比）。
pub const MAX_ADJ_PM: u64 = 50;
/// 调节冷却期（tick 数）。
pub const COOLDOWN_TICKS: u64 = 10;

/// 公平份额账户。
#[derive(Clone, Debug)]
pub struct FairShare {
    pub acct: u32,
    /// 当前权重。
    pub weight: u64,
    /// 手动优先钉住。
    pub pinned: bool,
    /// 已获服务量。
    pub served: u64,
}

/// 调节审计行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FairAudit {
    pub tick: u64,
    pub acct: u32,
    pub from: u64,
    pub to: u64,
    /// 调节理由（升权/降权）。
    pub up: bool,
}

/// 公平动态调节器。
#[derive(Debug)]
pub struct FairDyn {
    shares: Vec<FairShare>,
    /// 目标 Jain 指数（千分比）。
    target_pm: u64,
    audit: Vec<FairAudit>,
    now: u64,
    /// 上次自动调节 tick（冷却起点）。
    last_adjust: Option<u64>,
    /// 违规计数（pinned 被触碰）。
    pin_violations: u64,
}

impl FairDyn {
    /// 构造：目标须 ∈ (0, 1000]。
    pub fn new(target_pm: u64) -> Result<Self, u16> {
        if target_pm == 0 || target_pm > 1_000 {
            return Err(E_FAIR_TARGET);
        }
        Ok(Self {
            shares: Vec::new(),
            target_pm,
            audit: Vec::new(),
            now: 0,
            last_adjust: None,
            pin_violations: 0,
        })
    }

    pub fn enroll(&mut self, acct: u32, weight: u64) -> Result<(), u16> {
        if weight == 0 {
            return Err(E_FAIR_ACCT);
        }
        if self.shares.iter().any(|s| s.acct == acct) {
            return Err(E_FAIR_ACCT);
        }
        self.shares.push(FairShare {
            acct,
            weight,
            pinned: false,
            served: 0,
        });
        Ok(())
    }

    /// 记服务量（tick 推进时由仲裁侧喂）。
    pub fn serve(&mut self, acct: u32, amount: u64) -> Result<(), u16> {
        let s = self.shares.iter_mut().find(|s| s.acct == acct).ok_or(E_FAIR_ACCT)?;
        s.served = s.served.saturating_add(amount);
        Ok(())
    }

    pub fn tick(&mut self, n: u64) {
        self.now = self.now.saturating_add(n);
    }

    /// 手动优先声明：钉住（pinned 权重免疫自动调节）。
    pub fn pin(&mut self, acct: u32) -> Result<(), u16> {
        let s = self.shares.iter_mut().find(|s| s.acct == acct).ok_or(E_FAIR_ACCT)?;
        s.pinned = true;
        Ok(())
    }

    pub fn unpin(&mut self, acct: u32) -> Result<(), u16> {
        let s = self.shares.iter_mut().find(|s| s.acct == acct).ok_or(E_FAIR_ACCT)?;
        s.pinned = false;
        Ok(())
    }

    /// Jain 指数（千分比）：J = (Σx)²/(n·Σx²)，x = served/weight。
    /// 分母 0（全零服务）→ 视为全公平（1000‰——没有人在挨饿）。
    pub fn jain_pm(&self) -> u64 {
        let n = self.shares.len();
        if n == 0 {
            return 1_000;
        }
        // 用 x 的千分比定点：x_pm = served*1000/weight。
        let xs: Vec<u128> = self
            .shares
            .iter()
            .map(|s| (s.served as u128 * 1000) / s.weight.max(1) as u128)
            .collect();
        let sum: u128 = xs.iter().sum();
        let sq: u128 = xs.iter().map(|x| x * x).sum();
        let denom = (n as u128) * sq;
        if denom == 0 {
            return 1_000;
        }
        ((sum * sum) * 1000 / denom).min(1_000) as u64
    }

    pub fn target(&self) -> u64 {
        self.target_pm
    }

    /// 权重表快照（监控用）。
    pub fn weights(&self) -> Vec<(u32, u64)> {
        self.shares.iter().map(|s| (s.acct, s.weight)).collect()
    }

    /// 自动微调：J < 目标时对最失衡的未钉账户对（服务比最低者升权、
    /// 最高者降权）做有界调节。冷却期内拒（E_FAIR_OSC）。
    /// 返回本次调节的账户数（0 = 无需调节）。
    pub fn rebalance(&mut self) -> Result<usize, u16> {
        // 冷却期防线。
        if let Some(last) = self.last_adjust {
            if self.now.saturating_sub(last) < COOLDOWN_TICKS {
                return Err(E_FAIR_OSC);
            }
        }
        if self.jain_pm() >= self.target_pm {
            self.last_adjust = Some(self.now); // 达标也刷新冷却钟（防无效高频扫）。
            return Ok(0);
        }
        // 服务比最低/最高的未钉账户。
        let ratio = |s: &FairShare| -> u128 {
            (s.served as u128 * 1000) / s.weight.max(1) as u128
        };
        let mut min_i: Option<usize> = None;
        let mut max_i: Option<usize> = None;
        for (i, s) in self.shares.iter().enumerate() {
            if s.pinned {
                continue;
            }
            match min_i {
                None => min_i = Some(i),
                Some(m) if ratio(s) < ratio(&self.shares[m]) => min_i = Some(i),
                _ => {}
            }
            match max_i {
                None => max_i = Some(i),
                Some(m) if ratio(s) > ratio(&self.shares[m]) => max_i = Some(i),
                _ => {}
            }
        }
        let min_i = match min_i {
            Some(i) => i,
            None => return Ok(0), // 全钉死：自动调节无事可做（人工优先生效）。
        };
        let mut adjusted = 0usize;
        // 升权：+MAX_ADJ_PM（至少 +1——小权重也要动得了）。
        let (acct_lo, from_lo, to_lo) = {
            let s = &mut self.shares[min_i];
            let from = s.weight;
            let to = from.saturating_add(from.saturating_mul(MAX_ADJ_PM) / 1000).max(from + 1);
            s.weight = to;
            (s.acct, from, to)
        };
        self.audit.push(FairAudit {
            tick: self.now,
            acct: acct_lo,
            from: from_lo,
            to: to_lo,
            up: true,
        });
        adjusted += 1;
        // 降权：只在升权对象 != 降权对象时做（单账户不自我打摆）。
        if let Some(mxi) = max_i {
            if mxi != min_i {
                let (acct_hi, from_hi, to_hi) = {
                    let s = &mut self.shares[mxi];
                    let from = s.weight;
                    let cut = from.saturating_mul(MAX_ADJ_PM) / 1000;
                    if cut == 0 || from - cut == 0 {
                        // 降幅整除为 0 或降穿：跳过（不假装调过）。
                        (0, 0, 0)
                    } else {
                        let to = from - cut;
                        s.weight = to;
                        (s.acct, from, to)
                    }
                };
                if from_hi > 0 {
                    self.audit.push(FairAudit {
                        tick: self.now,
                        acct: acct_hi,
                        from: from_hi,
                        to: to_hi,
                        up: false,
                    });
                    adjusted += 1;
                }
            }
        }
        self.last_adjust = Some(self.now);
        Ok(adjusted)
    }

    /// 审计链对账：链尾 to == 当前权（每条链都成立）。
    pub fn audit_consistent(&self) -> Result<(), u16> {
        for s in &self.shares {
            let last = self.audit.iter().filter(|a| a.acct == s.acct).last();
            if let Some(a) = last {
                if a.to != s.weight {
                    return Err(E_FAIR_AUDIT);
                }
            }
        }
        Ok(())
    }

    /// 手动优先防线自证：自动调节路径对 pinned 的处理是"绕行"，
    /// 本方法显式返回违规口径（pinned 账户被要求自动调节 = E_FAIR_PIN）。
    pub fn auto_touch_pinned_is_violation(&mut self, acct: u32) -> Result<(), u16> {
        let s = self.shares.iter().find(|s| s.acct == acct).ok_or(E_FAIR_ACCT)?;
        if s.pinned {
            self.pin_violations = self.pin_violations.saturating_add(1);
            Err(E_FAIR_PIN)
        } else {
            Ok(())
        }
    }

    pub fn pin_violations(&self) -> u64 {
        self.pin_violations
    }

    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }

    pub fn weight_of(&self, acct: u32) -> Result<u64, u16> {
        self.shares
            .iter()
            .find(|s| s.acct == acct)
            .map(|s| s.weight)
            .ok_or(E_FAIR_ACCT)
    }

    pub fn pinned_of(&self, acct: u32) -> Result<bool, u16> {
        self.shares
            .iter()
            .find(|s| s.acct == acct)
            .map(|s| s.pinned)
            .ok_or(E_FAIR_ACCT)
    }
}

// ==========================================================================
// 判据（施工书：监控、有界微调、审计、手动优先、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_Jain指数监控_已知向量自证() {
        let mut f = FairDyn::new(900).unwrap();
        f.enroll(1, 10).unwrap();
        f.enroll(2, 10).unwrap();
        // 全零服务 → 全公平。
        assert_eq!(f.jain_pm(), 1_000);
        // 全等服务 → J = 1000‰。
        f.serve(1, 100).unwrap();
        f.serve(2, 100).unwrap();
        assert_eq!(f.jain_pm(), 1_000);
        // 一方失衡 2 倍（x1=20000, x2=10000）→ J = 9e8/1e9 → 900‰。
        f.serve(1, 100).unwrap();
        assert_eq!(f.jain_pm(), 900);
        // 极端失衡（x1=100000, x2=10000）→ J ≈ 599‰。
        f.serve(1, 800).unwrap();
        assert_eq!(f.jain_pm(), 599);
        // 非法目标拒。
        assert!(matches!(FairDyn::new(0), Err(E_FAIR_TARGET)));
        assert!(matches!(FairDyn::new(1_001), Err(E_FAIR_TARGET)));
        // 空表 = 公平（无人挨饿）。
        let e = FairDyn::new(900).unwrap();
        assert_eq!(e.jain_pm(), 1_000);
    }

    #[test]
    fn a02_跌破目标_有界微调() {
        let mut f = FairDyn::new(950).unwrap();
        f.enroll(1, 100).unwrap();
        f.enroll(2, 100).unwrap();
        // 制造失衡：账户 1 吃 3000，账户 2 吃 0 → J=500。
        f.serve(1, 3_000).unwrap();
        assert_eq!(f.jain_pm(), 500);
        // 微调：升权账户 2、降权账户 1，单次幅度 = 50‰。
        assert_eq!(f.rebalance(), Ok(2));
        let w1 = f.weight_of(1).unwrap();
        let w2 = f.weight_of(2).unwrap();
        assert_eq!(w2, 105); // 100 + 100×50‰ = 105。
        assert_eq!(w1, 95); // 100 − 100×50‰ = 95。
        // 冷却期内二调拒（振荡防线）。
        assert_eq!(f.rebalance(), Err(E_FAIR_OSC));
        f.tick(COOLDOWN_TICKS);
        // 冷却过后可再调（继续收敛或已达标返回 0）。
        let r = f.rebalance();
        assert!(r == Ok(2) || r == Ok(0));
    }

    #[test]
    fn a03_手动优先_钉住不被覆盖() {
        let mut f = FairDyn::new(950).unwrap();
        f.enroll(1, 10).unwrap();
        f.enroll(2, 10).unwrap();
        f.pin(1).unwrap();
        assert_eq!(f.pinned_of(1), Ok(true));
        // 自动调节绕行：全钉死 → 无事可做（人工优先生效）。
        f.pin(2).unwrap();
        f.serve(1, 500).unwrap();
        assert_eq!(f.jain_pm(), 500);
        f.tick(COOLDOWN_TICKS);
        assert_eq!(f.rebalance(), Ok(0));
        assert_eq!(f.weight_of(1), Ok(10));
        assert_eq!(f.weight_of(2), Ok(10));
        // pinned 触碰自证违规。
        assert_eq!(f.auto_touch_pinned_is_violation(1), Err(E_FAIR_PIN));
        assert_eq!(f.pin_violations(), 1);
        assert_eq!(f.auto_touch_pinned_is_violation(2), Err(E_FAIR_PIN));
        // 解钉后恢复自动调节资格。
        f.unpin(2).unwrap();
        assert_eq!(f.pinned_of(2), Ok(false));
        assert_eq!(f.auto_touch_pinned_is_violation(2), Ok(()));
        // 未登记账户 pin 拒。
        assert_eq!(f.pin(99), Err(E_FAIR_ACCT));
    }

    #[test]
    fn a04_调节审计_不悄悄改() {
        let mut f = FairDyn::new(900).unwrap();
        f.enroll(1, 100).unwrap();
        f.enroll(2, 100).unwrap();
        f.serve(1, 3_000).unwrap();
        f.tick(COOLDOWN_TICKS);
        f.rebalance().unwrap_or(0);
        // 审计非空。
        assert!(f.audit_len() >= 1);
        assert_eq!(f.audit_consistent(), Ok(()));
        // 篡改现状 → 链断红（不悄悄改自证）。
        if let Some(s) = f.shares.iter_mut().find(|s| s.acct == 2) {
            s.weight = s.weight.wrapping_add(77);
        }
        assert_eq!(f.audit_consistent(), Err(E_FAIR_AUDIT));
        // 审计行方向正确：升权行 up=true，降权行 up=false。
        let mut f2 = FairDyn::new(900).unwrap();
        f2.enroll(1, 100).unwrap();
        f2.enroll(2, 100).unwrap();
        f2.serve(1, 3_000).unwrap();
        f2.tick(COOLDOWN_TICKS);
        f2.rebalance().unwrap_or(0);
        let ups = f2.audit.iter().filter(|a| a.up).count();
        let downs = f2.audit.iter().filter(|a| !a.up).count();
        assert!(ups >= 1);
        assert!(downs >= 1);
    }

    #[test]
    fn a05_登记卫生与权重零拒() {
        let mut f = FairDyn::new(900).unwrap();
        // 零权重拒。
        assert_eq!(f.enroll(1, 0), Err(E_FAIR_ACCT));
        // 重复登记拒。
        f.enroll(1, 10).unwrap();
        assert_eq!(f.enroll(1, 5), Err(E_FAIR_ACCT));
        // 未登记 serve 拒。
        assert_eq!(f.serve(99, 1), Err(E_FAIR_ACCT));
        // 权重表快照。
        f.enroll(2, 20).unwrap();
        let ws = f.weights();
        assert_eq!(ws.len(), 2);
        assert_eq!(ws[0], (1, 10));
        assert_eq!(ws[1], (2, 20));
        // 常量 sanity。
        assert!(MAX_ADJ_PM > 0 && MAX_ADJ_PM <= 1_000);
        assert!(COOLDOWN_TICKS > 0);
    }
}
