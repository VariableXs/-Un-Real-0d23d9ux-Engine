//! CGPU-F0437 · 仲裁策略插件化（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 策略插件：策略注册器（新仲裁策略以插件接入——统一策略接口）；验证
//!   门槛（新策略必须过影子运行 F0388 对比+全域 fuzz——达标才可启用）；
//!   策略生命周期（注册/验证/启用/停用/卸载全状态机）；内置策略三件
//!   （v1 静态/v2 WFQ/混合——作为首批插件）。判据：注册器、验证门槛、
//!   生命周期、内置三件、判据。
//!
//! 设计纪律：
//! - 统一策略接口：`ArbPolicy` trait——仲裁域只对接口编程，新策略
//!   零侵入接入（注册器持有 trait 对象账）。
//! - 验证门槛两把锁：影子运行对比（同输入新旧策略决策差 ≤ 带）+
//!   全域 fuzz 通过（三案例类别覆盖）——双达标才准 Registered→Verified。
//! - 生命周期闭集：Registered→Verified→Enabled→Disabled→Unloaded，
//!   非法跃迁一律拒（禁跳级启用——未验证策略直接上线 = 假验证）。
//! - 内置三件首批注册（v1 静态/v2 WFQ/混合）——它们也要走同一验证
//!   门槛（内置不豁免——规则面前策略平等）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0E80 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0E80，全仓独占）。
pub const SEG_BASE: u16 = 0x0E80;

pub const E_PLG_NAME: u16 = SEG_BASE + 1; // 策略名非法（空/重名）
pub const E_PLG_STATE: u16 = SEG_BASE + 2; // 生命周期非法跃迁
pub const E_PLG_GATE: u16 = SEG_BASE + 3; // 验证门槛未过（禁启用）
pub const E_PLG_FUZZ: u16 = SEG_BASE + 4; // 全域 fuzz 未覆盖
pub const E_PLG_INUSE: u16 = SEG_BASE + 5; // 启用中策略禁卸载
pub const E_PLG_DECIDE: u16 = SEG_BASE + 6; // 策略决策非法（索引越界）

/// 影子对比差带（千分位——新旧决策份额差 ≤ 5%）。
pub const SHADOW_BAND_PM: u64 = 50;

/// 策略生命周期状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlgState {
    Registered,
    Verified,
    Enabled,
    Disabled,
    Unloaded,
}

impl PlgState {
    pub fn code(self) -> u8 {
        match self {
            PlgState::Registered => 0,
            PlgState::Verified => 1,
            PlgState::Enabled => 2,
            PlgState::Disabled => 3,
            PlgState::Unloaded => 4,
        }
    }
}

/// 统一策略接口：给定各账户权重与总预算 → 各账户份额。
/// 纯函数（同输入同输出）——策略不做 IO 不持有秘密。
pub trait ArbPolicy {
    fn name(&self) -> &'static str;
    /// shares 与 weights 同长，份额千分比合计 ≤ 1000。
    fn decide(&self, weights: &[u64], served: &[u64], budget_pm: u64) -> Vec<u64>;
}

/// 内置策略①：v1 静态（按权重静态切分——最朴素的基线）。
pub struct StaticPolicy;

impl ArbPolicy for StaticPolicy {
    fn name(&self) -> &'static str {
        "v1-static"
    }

    fn decide(&self, weights: &[u64], _served: &[u64], budget_pm: u64) -> Vec<u64> {
        let sum: u64 = weights.iter().sum();
        if sum == 0 {
            return alloc::vec![0; weights.len()];
        }
        weights
            .iter()
            .map(|w| *w * budget_pm / sum)
            .collect()
    }
}

/// 内置策略②：v2 WFQ（加权公平队列——按权重/已服务的归一公平）。
pub struct WfqPolicy;

impl ArbPolicy for WfqPolicy {
    fn name(&self) -> &'static str {
        "v2-wfq"
    }

    fn decide(&self, weights: &[u64], served: &[u64], budget_pm: u64) -> Vec<u64> {
        // 归一化份额 x = served/weight；缺额向 x 最低者倾斜。
        let n = weights.len();
        let mut xs: Vec<u128> = (0..n)
            .map(|i| {
                if weights[i] == 0 {
                    u128::MAX
                } else {
                    (served[i] as u128 * 1000) / weights[i] as u128
                }
            })
            .collect();
        let sum: u128 = xs.iter().filter(|x| **x != u128::MAX).sum();
        let live = xs.iter().filter(|x| **x != u128::MAX).count();
        if live == 0 {
            return alloc::vec![0; n];
        }
        let avg = sum / live as u128;
        // 需求度 = max(0, avg - x)；按需求度分配。
        let needs: Vec<u128> = xs
            .iter()
            .map(|x| if *x == u128::MAX { 0 } else { avg.saturating_sub(*x) })
            .collect();
        let nsum: u128 = needs.iter().sum();
        if nsum == 0 {
            return StaticPolicy.decide(weights, served, budget_pm);
        }
        let out: Vec<u64> = needs
            .iter()
            .map(|nd| (*nd * budget_pm as u128 / nsum) as u64)
            .collect();
        xs.clear();
        out
    }
}

/// 内置策略③：混合（静态打底 + WFQ 修正，各半）。
pub struct HybridPolicy;

impl ArbPolicy for HybridPolicy {
    fn name(&self) -> &'static str {
        "hybrid"
    }

    fn decide(&self, weights: &[u64], served: &[u64], budget_pm: u64) -> Vec<u64> {
        let a = StaticPolicy.decide(weights, served, budget_pm / 2);
        let b = WfqPolicy.decide(weights, served, budget_pm - budget_pm / 2);
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| x.saturating_add(*y))
            .collect()
    }
}

/// 策略插件账目。
#[derive(Debug)]
pub struct PolicyRec {
    /// 插件名（唯一键）。
    pub name: &'static str,
    /// 生命周期状态。
    pub state: PlgState,
    /// 影子对比最大差（千分位）。
    pub shadow_diff_pm: u64,
    /// 全域 fuzz 是否通过。
    pub fuzz_ok: bool,
}

/// 策略注册器。
#[derive(Debug, Default)]
pub struct PolicyRegistry {
    recs: Vec<PolicyRec>,
    /// 当前启用的策略名。
    pub active: Option<&'static str>,
}

impl PolicyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册新策略（空名/重名拒）。
    pub fn register(&mut self, name: &'static str) -> Result<(), u16> {
        if name.is_empty() {
            return Err(E_PLG_NAME);
        }
        if self.recs.iter().any(|r| r.name == name && r.state != PlgState::Unloaded) {
            return Err(E_PLG_NAME);
        }
        self.recs.push(PolicyRec {
            name,
            state: PlgState::Registered,
            shadow_diff_pm: u64::MAX,
            fuzz_ok: false,
        });
        Ok(())
    }

    fn rec_idx(&self, name: &str) -> Result<usize, u16> {
        self.recs
            .iter()
            .position(|r| r.name == name && r.state != PlgState::Unloaded)
            .ok_or(E_PLG_NAME)
    }

    /// 录入影子对比结果（差 ≤ 带才记录为过）。
    pub fn record_shadow(&mut self, name: &str, diff_pm: u64) -> Result<bool, u16> {
        let idx = self.rec_idx(name)?;
        let r = &mut self.recs[idx];
        r.shadow_diff_pm = diff_pm;
        Ok(diff_pm <= SHADOW_BAND_PM)
    }

    /// 录入全域 fuzz 结果。
    pub fn record_fuzz(&mut self, name: &str, ok: bool) -> Result<(), u16> {
        let idx = self.rec_idx(name)?;
        let r = &mut self.recs[idx];
        r.fuzz_ok = ok;
        Ok(())
    }

    /// 验证门槛：Registered→Verified（双达标才准）。
    pub fn verify(&mut self, name: &str) -> Result<(), u16> {
        let i = self.rec_idx(name)?;
        let r = &self.recs[i];
        if r.state != PlgState::Registered {
            return Err(E_PLG_STATE);
        }
        let shadow_ok = r.shadow_diff_pm != u64::MAX && r.shadow_diff_pm <= SHADOW_BAND_PM;
        if !shadow_ok || !r.fuzz_ok {
            return Err(E_PLG_GATE);
        }
        self.recs[i].state = PlgState::Verified;
        Ok(())
    }

    /// 启用（仅 Verified 可启用；同刻单启用）。
    pub fn enable(&mut self, name: &'static str) -> Result<(), u16> {
        let i = self.rec_idx(name)?;
        if self.recs[i].state != PlgState::Verified {
            return Err(E_PLG_STATE);
        }
        if self.active.is_some() {
            return Err(E_PLG_STATE); // 单仲裁器单策略（多策略协同属 F0443 预留）。
        }
        self.recs[i].state = PlgState::Enabled;
        self.active = Some(name);
        Ok(())
    }

    /// 停用（Enabled→Disabled，释放启用位）。
    pub fn disable(&mut self, name: &str) -> Result<(), u16> {
        let i = self.rec_idx(name)?;
        if self.recs[i].state != PlgState::Enabled {
            return Err(E_PLG_STATE);
        }
        self.recs[i].state = PlgState::Disabled;
        self.active = None;
        Ok(())
    }

    /// 卸载（仅 Disabled/Registered 可卸；启用中拒）。
    pub fn unload(&mut self, name: &str) -> Result<(), u16> {
        let i = self.rec_idx(name)?;
        if self.recs[i].state == PlgState::Enabled {
            return Err(E_PLG_INUSE);
        }
        self.recs[i].state = PlgState::Unloaded;
        Ok(())
    }

    /// 重新启用路径：Disabled→（重新验证）→Verified→Enabled。
    pub fn reverify(&mut self, name: &str) -> Result<(), u16> {
        let i = self.rec_idx(name)?;
        if self.recs[i].state != PlgState::Disabled {
            return Err(E_PLG_STATE);
        }
        // 停用过的策略重新启用必须重走门槛（影子+fuzz 清零重录）。
        self.recs[i].shadow_diff_pm = u64::MAX;
        self.recs[i].fuzz_ok = false;
        self.recs[i].state = PlgState::Registered;
        Ok(())
    }

    /// 用策略对象裁决（只认当前启用策略；未启用拒）。
    pub fn decide_with(
        &self,
        weights: &[u64],
        served: &[u64],
        budget_pm: u64,
        policy: &dyn ArbPolicy,
    ) -> Result<Vec<u64>, u16> {
        let act = self.active.ok_or(E_PLG_STATE)?;
        if policy.name() != act {
            return Err(E_PLG_DECIDE);
        }
        let shares = policy.decide(weights, served, budget_pm);
        if shares.len() != weights.len() {
            return Err(E_PLG_DECIDE);
        }
        Ok(shares)
    }

    pub fn state_of(&self, name: &str) -> Result<PlgState, u16> {
        // 优先活跃记录；无活跃时回退 Unloaded 记录（卸载态可查）。
        self.recs
            .iter()
            .find(|r| r.name == name && r.state != PlgState::Unloaded)
            .or_else(|| self.recs.iter().find(|r| r.name == name))
            .map(|r| r.state)
            .ok_or(E_PLG_NAME)
    }

    pub fn count(&self) -> usize {
        self.recs.iter().filter(|r| r.state != PlgState::Unloaded).count()
    }
}

// ==========================================================================
// 判据（施工书：注册器、验证门槛、生命周期、内置三件、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_注册器_内置三件首批接入() {
        let mut rg = PolicyRegistry::new();
        assert_eq!(rg.register("v1-static"), Ok(()));
        assert_eq!(rg.register("v2-wfq"), Ok(()));
        assert_eq!(rg.register("hybrid"), Ok(()));
        assert_eq!(rg.count(), 3);
        // 空名拒。
        assert_eq!(rg.register(""), Err(E_PLG_NAME));
        // 重名拒。
        assert_eq!(rg.register("v1-static"), Err(E_PLG_NAME));
        assert_eq!(rg.state_of("v1-static"), Ok(PlgState::Registered));
    }

    #[test]
    fn a02_验证门槛_双达标才可启用() {
        let mut rg = PolicyRegistry::new();
        rg.register("v2-wfq").ok();
        // 未录任何结果 → verify 拒（假验证防线）。
        assert_eq!(rg.verify("v2-wfq"), Err(E_PLG_GATE));
        // 影子超带。
        assert_eq!(rg.record_shadow("v2-wfq", SHADOW_BAND_PM + 1), Ok(false));
        assert_eq!(rg.record_fuzz("v2-wfq", true), Ok(()));
        assert_eq!(rg.verify("v2-wfq"), Err(E_PLG_GATE));
        // 影子达标但 fuzz 缺。
        rg.record_shadow("v2-wfq", 10).ok();
        assert_eq!(rg.record_fuzz("v2-wfq", false), Ok(()));
        assert_eq!(rg.verify("v2-wfq"), Err(E_PLG_GATE));
        // 双达标 → 过。
        rg.record_fuzz("v2-wfq", true).ok();
        assert_eq!(rg.verify("v2-wfq"), Ok(()));
        assert_eq!(rg.state_of("v2-wfq"), Ok(PlgState::Verified));
        // 未验证直接启用拒（跳级防线）。
        rg.register("hybrid").ok();
        assert_eq!(rg.enable("hybrid"), Err(E_PLG_STATE));
    }

    #[test]
    fn a03_生命周期_全状态机闭集() {
        let mut rg = PolicyRegistry::new();
        rg.register("v1-static").ok();
        rg.record_shadow("v1-static", 0).ok();
        rg.record_fuzz("v1-static", true).ok();
        rg.verify("v1-static").ok();
        assert_eq!(rg.enable("v1-static"), Ok(()));
        assert_eq!(rg.active, Some("v1-static"));
        // 启用中不可卸载。
        assert_eq!(rg.unload("v1-static"), Err(E_PLG_INUSE));
        // 单仲裁器单启用：第二策略已验证也拒。
        rg.register("v2-wfq").ok();
        rg.record_shadow("v2-wfq", 0).ok();
        rg.record_fuzz("v2-wfq", true).ok();
        rg.verify("v2-wfq").ok();
        assert_eq!(rg.enable("v2-wfq"), Err(E_PLG_STATE));
        // 停用 → 卸载。
        assert_eq!(rg.disable("v1-static"), Ok(()));
        assert_eq!(rg.active, None);
        assert_eq!(rg.unload("v1-static"), Ok(()));
        assert_eq!(rg.state_of("v1-static"), Ok(PlgState::Unloaded));
        // 卸载后可重名注册（槽位回收）。
        assert_eq!(rg.register("v1-static"), Ok(()));
    }

    #[test]
    fn a04_内置三件_决策语义() {
        let w = [10u64, 10, 10];
        let s = [0u64, 0, 100];
        // v1 静态：均分。
        let a = StaticPolicy.decide(&w, &s, 900);
        assert_eq!(a, vec![300, 300, 300]);
        // v2 WFQ：账户 3 已服务多 → 需求向 1/2 倾斜。
        let b = WfqPolicy.decide(&w, &s, 900);
        assert_eq!(b[0], b[1]);
        assert!(b[0] > b[2], "WFQ 应向低服务比倾斜");
        // 混合：介于两者之间的确定性合成。
        let c = HybridPolicy.decide(&w, &s, 900);
        let sum: u64 = c.iter().sum();
        assert_eq!(sum, 900);
        // 全零权重：v1 不炸（0 份额）。
        let z = StaticPolicy.decide(&[0, 0], &[0, 0], 500);
        assert_eq!(z, vec![0, 0]);
    }

    #[test]
    fn a05_再启用_重走门槛_裁决绑定() {
        let mut rg = PolicyRegistry::new();
        rg.register("hybrid").ok();
        rg.record_shadow("hybrid", 5).ok();
        rg.record_fuzz("hybrid", true).ok();
        rg.verify("hybrid").ok();
        rg.enable("hybrid").ok();
        rg.disable("hybrid").ok();
        // 再启用前必须重走门槛（影子/fuzz 清零）。
        rg.reverify("hybrid").ok();
        assert_eq!(rg.state_of("hybrid"), Ok(PlgState::Registered));
        assert_eq!(rg.verify("hybrid"), Err(E_PLG_GATE));
        rg.record_shadow("hybrid", 5).ok();
        rg.record_fuzz("hybrid", true).ok();
        rg.verify("hybrid").ok();
        rg.enable("hybrid").ok();
        // 裁决绑定：启用 hybrid 时拿 WFQ 决策 → 拒（名不符实）。
        assert_eq!(
            rg.decide_with(&[1], &[0], 100, &WfqPolicy),
            Err(E_PLG_DECIDE)
        );
        let ok = rg.decide_with(&[10, 10], &[0, 0], 500, &HybridPolicy);
        assert!(ok.is_ok());
        // 未启用任何策略 → 裁决拒。
        rg.disable("hybrid").ok();
        assert_eq!(
            rg.decide_with(&[1], &[0], 100, &HybridPolicy),
            Err(E_PLG_STATE)
        );
    }
}
