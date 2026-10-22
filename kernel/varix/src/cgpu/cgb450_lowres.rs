//! CGPU-F0450 · 低资源设备预算（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 低资源：低端档预算模型简化（弱 CPU/核显设备的预算模型轻量版——
//!   仲裁开销同步降）；配额压缩（账户数/粒度在低端档的简化）；质量
//!   预期管理（低端档的 80 帧承诺适用范围诚实声明——按 O 域认证矩阵）；
//!   低端档专属配方。判据：模型简化、配额压缩、预期诚实、专属配方、判据。
//!
//! 设计纪律：
//! - 模型简化三件事：预算粒度千分位 → 百分位（计算量降一位）、无
//!   漂移/季节等高级分析（统计管道只留总账）、仲裁策略固定静态档
//!   （轻量版的确定性）。
//! - 配额压缩：账户数上限减半、窗口内申请上限减半——压缩配方常量
//!   入册（低端档 ≠ 随意砍，是预定义的确定性收缩）。
//! - 预期诚实：80 帧承诺在低端档的适用范围必须显式携带「受限」标注
//!   （帧承诺结构带 honest 字段——低端档承诺帧率按 O 域认证矩阵打折）。
//! - 专属配方：低端档独有参数集（粒度/上限/承诺）一表可查。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0F50 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0F50，全仓独占）。
pub const SEG_BASE: u16 = 0x0F50;

pub const E_LR_ACCOUNTS: u16 = SEG_BASE + 1; // 账户数超低端档配额
pub const E_LR_TIER: u16 = SEG_BASE + 2; // 设备档非法
pub const E_LR_GRANULARITY: u16 = SEG_BASE + 3; // 粒度违反（低端档禁千分位精度）
pub const E_LR_HONEST: u16 = SEG_BASE + 4; // 预期不诚实（受限未标注）

/// 低端档配额压缩配方（相对全量档）。
pub const LOWEND_MAX_ACCOUNTS: usize = 4;
pub const FULL_MAX_ACCOUNTS: usize = 8;
pub const LOWEND_MAX_REQ_PER_FRAME: u64 = 200;
pub const FULL_MAX_REQ_PER_FRAME: u64 = 400;
/// 粒度收缩：千分位 → 百分位。
pub const LOWEND_GRANULARITY: u64 = 100;
pub const FULL_GRANULARITY: u64 = 1;

/// 设备档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceTier {
    LowEnd,
    Full,
}

/// 帧率承诺（诚实标注结构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FpsPromise {
    pub fps: u32,
    /// 受限标注：低端档恒 true（承诺适用范围诚实声明）。
    pub restricted: bool,
    /// 适用范围说明（O 域认证矩阵口径）。
    pub scope: &'static str,
}

/// 低资源预算台。
#[derive(Debug)]
pub struct LowResBudget {
    tier: DeviceTier,
    accounts: Vec<(u32, u64)>,
    pub max_accounts: usize,
    pub max_req_frame: u64,
    pub granularity: u64,
    frame_requests: u64,
}

impl LowResBudget {
    pub fn new(tier: DeviceTier) -> Self {
        let (a, r, g) = match tier {
            DeviceTier::LowEnd => (LOWEND_MAX_ACCOUNTS, LOWEND_MAX_REQ_PER_FRAME, LOWEND_GRANULARITY),
            DeviceTier::Full => (FULL_MAX_ACCOUNTS, FULL_MAX_REQ_PER_FRAME, FULL_GRANULARITY),
        };
        Self {
            tier,
            accounts: Vec::new(),
            max_accounts: a,
            max_req_frame: r,
            granularity: g,
            frame_requests: 0,
        }
    }

    pub fn tier(&self) -> DeviceTier {
        self.tier
    }

    /// 开户：低端档配额压缩（超压缩配额拒）。
    pub fn open(&mut self, acct: u32) -> Result<(), u16> {
        if self.accounts.len() >= self.max_accounts {
            return Err(E_LR_ACCOUNTS);
        }
        if self.accounts.iter().any(|(a, _)| *a == acct) {
            return Err(E_LR_ACCOUNTS);
        }
        self.accounts.push((acct, 0));
        Ok(())
    }

    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }

    /// 份额提交：低端档粒度强制百分位（千分位精度在低端档违规）。
    pub fn grant(&mut self, acct: u32, share_pm: u64) -> Result<u64, u16> {
        let i = self.accounts.iter().position(|(a, _)| *a == acct).ok_or(E_LR_ACCOUNTS)?;
        // 帧申请上限。
        if self.frame_requests >= self.max_req_frame {
            return Err(E_LR_ACCOUNTS);
        }
        self.frame_requests += 1;
        // 粒度闸：低端档份额必须是粒度倍数。
        if self.tier == DeviceTier::LowEnd && share_pm % LOWEND_GRANULARITY != 0 {
            return Err(E_LR_GRANULARITY);
        }
        if self.tier == DeviceTier::Full && share_pm % FULL_GRANULARITY != 0 {
            return Err(E_LR_GRANULARITY);
        }
        let bal = &mut self.accounts[i].1;
        *bal = bal.saturating_add(share_pm);
        Ok(*bal)
    }

    /// 帧复位。
    pub fn reset_frame(&mut self) {
        self.frame_requests = 0;
    }

    /// 帧率承诺（预期诚实）：低端档 30 帧且受限标注，全量档 80 帧不限。
    pub fn fps_promise(tier: DeviceTier) -> FpsPromise {
        match tier {
            DeviceTier::LowEnd => FpsPromise {
                fps: 30,
                restricted: true,
                scope: "仅认证矩阵低端档清单内场景（80 帧承诺不适用）",
            },
            DeviceTier::Full => FpsPromise {
                fps: 80,
                restricted: false,
                scope: "全场景",
            },
        }
    }

    /// 诚实校验：低端档的承诺必须带受限标注。
    pub fn promise_honest(p: &FpsPromise, tier: DeviceTier) -> Result<(), u16> {
        match tier {
            DeviceTier::LowEnd if !p.restricted => Err(E_LR_HONEST),
            _ => Ok(()),
        }
    }
}

// ==========================================================================
// 判据（施工书：模型简化、配额压缩、预期诚实、专属配方、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_模型简化_粒度收缩() {
        let mut lr = LowResBudget::new(DeviceTier::LowEnd);
        lr.open(1).unwrap();
        // 百分位粒度放行。
        assert_eq!(lr.grant(1, 100), Ok(100));
        assert_eq!(lr.grant(1, 300), Ok(400));
        // 千分位精度在低端档违规。
        assert_eq!(lr.grant(1, 123), Err(E_LR_GRANULARITY));
        // 全量档保留千分位精度。
        let mut f = LowResBudget::new(DeviceTier::Full);
        f.open(1).unwrap();
        assert_eq!(f.grant(1, 123), Ok(123));
        // 配方常量：粒度收缩 10 倍。
        assert_eq!(LOWEND_GRANULARITY / FULL_GRANULARITY, 100);
    }

    #[test]
    fn a02_配额压缩_账户与申请减半() {
        let mut lr = LowResBudget::new(DeviceTier::LowEnd);
        for i in 0..LOWEND_MAX_ACCOUNTS {
            assert_eq!(lr.open(i as u32), Ok(()));
        }
        // 超压缩配额拒。
        assert_eq!(lr.open(99), Err(E_LR_ACCOUNTS));
        assert_eq!(lr.account_count(), LOWEND_MAX_ACCOUNTS);
        // 重复开户拒。
        assert_eq!(lr.open(0), Err(E_LR_ACCOUNTS));
        // 申请上限减半。
        assert_eq!(FULL_MAX_REQ_PER_FRAME / LOWEND_MAX_REQ_PER_FRAME, 2);
        let mut lr2 = LowResBudget::new(DeviceTier::LowEnd);
        lr2.open(1).unwrap();
        for _ in 0..LOWEND_MAX_REQ_PER_FRAME {
            lr2.grant(1, 100).ok();
        }
        assert_eq!(lr2.grant(1, 100), Err(E_LR_ACCOUNTS));
        lr2.reset_frame();
        assert_eq!(lr2.grant(1, 100), Ok(20_100)); // 200×100 累计 + 本笔
    }

    #[test]
    fn a03_预期诚实_低端档承诺必须带受限标注() {
        let p = LowResBudget::fps_promise(DeviceTier::LowEnd);
        assert_eq!(p.fps, 30);
        assert!(p.restricted);
        assert!(!p.scope.is_empty());
        assert_eq!(LowResBudget::promise_honest(&p, DeviceTier::LowEnd), Ok(()));
        // 低端档无受限标注 = 预期不诚实。
        let lie = FpsPromise { fps: 80, restricted: false, scope: "全场景" };
        assert_eq!(LowResBudget::promise_honest(&lie, DeviceTier::LowEnd), Err(E_LR_HONEST));
        // 全量档 80 帧不受限。
        let f = LowResBudget::fps_promise(DeviceTier::Full);
        assert_eq!(f.fps, 80);
        assert!(!f.restricted);
        assert_eq!(LowResBudget::promise_honest(&f, DeviceTier::Full), Ok(()));
    }

    #[test]
    fn a04_专属配方_低端档参数表() {
        // 低端档独有参数集一表可查（常量自洽）。
        assert!(LOWEND_MAX_ACCOUNTS < FULL_MAX_ACCOUNTS);
        assert!(LOWEND_MAX_REQ_PER_FRAME < FULL_MAX_REQ_PER_FRAME);
        assert!(LOWEND_GRANULARITY > FULL_GRANULARITY);
        let lr = LowResBudget::new(DeviceTier::LowEnd);
        assert_eq!(lr.max_accounts, LOWEND_MAX_ACCOUNTS);
        assert_eq!(lr.max_req_frame, LOWEND_MAX_REQ_PER_FRAME);
        assert_eq!(lr.granularity, LOWEND_GRANULARITY);
        let f = LowResBudget::new(DeviceTier::Full);
        assert_eq!(f.max_accounts, FULL_MAX_ACCOUNTS);
    }

    #[test]
    fn a05_未知账户与帧复位() {
        let mut lr = LowResBudget::new(DeviceTier::LowEnd);
        // 未开户 grant 拒。
        assert_eq!(lr.grant(7, 100), Err(E_LR_ACCOUNTS));
        lr.open(7).unwrap();
        assert_eq!(lr.grant(7, 200), Ok(200));
        // 帧复位不清余额（余额是账、申请数是水位）。
        lr.reset_frame();
        assert_eq!(lr.grant(7, 100), Ok(300));
        assert_eq!(lr.account_count(), 1);
    }
}
