//! CGPU-F0218 · 帧图与省电模式联动（判据段 0x0740）
//! 省电模式的帧图适配：省电模式图变换（低功耗→图节点降级：少
//! Pass/低分辨率 Pass 替换/跳过非关键效果节点）、帧率上限联动（省
//! 电帧率上限 → 图执行频率约束）、功耗反馈闭环（功耗实测 → 图变
//! 换策略调整——自适应省电）。判据：图变换正确、帧率联动、功耗闭
//! 环、性能/功耗权衡表、用户可配。
//!
//! 诊断码段 0x0740（本模块独占：0x0740~0x074F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0740;

/// 省电帧率上限（×100）：30fps = 3000。
pub const POWERSAVE_FPS_CAP_CENT: u32 = 3000;

/// 功耗闭环采样窗口（帧）。
pub const POWER_LOOP_WINDOW: usize = 16;

/// 功耗下降目标（千分比）：省电模式须降 ≥ 200‰。
pub const POWER_SAVING_MIN_PERMILLE: u32 = 200;

/// 图节点能效档位（判据「图变换正确」的变换对象）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeTier {
    /// 全质节点（高耗）。
    Full,
    /// 降分辨率节点（中耗）。
    HalfRes,
    /// 跳过（零耗——非关键效果）。
    Skipped,
}

impl NodeTier {
    /// 档位号。
    pub fn ordinal(&self) -> u8 {
        match self {
            NodeTier::Full => 0,
            NodeTier::HalfRes => 1,
            NodeTier::Skipped => 2,
        }
    }
    /// 能耗权重（Full 100 / HalfRes 55 / Skipped 0，千分比 = ×10）。
    pub fn power_weight(&self) -> u32 {
        match self {
            NodeTier::Full => 1000,
            NodeTier::HalfRes => 550,
            NodeTier::Skipped => 0,
        }
    }
    /// 画质权重（Full 1000 / HalfRes 500 / Skipped 0）。
    pub fn quality_weight(&self) -> u32 {
        match self {
            NodeTier::Full => 1000,
            NodeTier::HalfRes => 500,
            NodeTier::Skipped => 0,
        }
    }
}

/// 可变换节点（判据「图变换正确」的输入）：节点 + 关键性。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PowerNode {
    /// 节点号。
    pub node: u32,
    /// 原始档位。
    pub base: NodeTier,
    /// 是否关键节点（关键节点只允许 HalfRes，不得 Skipped）。
    pub critical: bool,
}

impl PowerNode {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.node == 0 {
            return Err(SEG_BASE);
        }
        Ok(())
    }
}

/// 省电等级（判据「用户可配」与「图变换」的档位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SaveLevel {
    /// 不省电。
    Off,
    /// 轻度：非关键全质 → HalfRes。
    Light,
    /// 重度：非关键 → Skipped，关键 → HalfRes。
    Aggressive,
}

impl SaveLevel {
    /// 档位号。
    pub fn ordinal(&self) -> u8 {
        match self {
            SaveLevel::Off => 0,
            SaveLevel::Light => 1,
            SaveLevel::Aggressive => 2,
        }
    }
}

/// 省电图变换（判据「图变换正确」主判据）：按档位变换全部节点。
/// 约束：关键节点永不被 Skipped（画面缺主干 = 变换非法）。
pub fn transform_for_power(nodes: &[PowerNode], level: SaveLevel) -> Result<Vec<NodeTier>, u16> {
    if nodes.is_empty() {
        return Err(SEG_BASE + 1);
    }
    let mut out = Vec::new();
    for n in nodes {
        n.validate()?;
        let t = match level {
            SaveLevel::Off => n.base,
            SaveLevel::Light => match n.base {
                NodeTier::Full if !n.critical => NodeTier::HalfRes,
                other => other,
            },
            SaveLevel::Aggressive => {
                if n.critical {
                    NodeTier::HalfRes
                } else {
                    NodeTier::Skipped
                }
            }
        };
        if t == NodeTier::Skipped && n.critical {
            return Err(SEG_BASE + 2);
        }
        out.push(t);
    }
    Ok(out)
}

/// 功耗估算（判据「性能/功耗权衡表」核心量）：变换后全图相对功耗
/// 千分比（基准 = 全 Full）。
pub fn estimated_power_permille(tiers: &[NodeTier]) -> Result<u32, u16> {
    if tiers.is_empty() {
        return Err(SEG_BASE + 3);
    }
    let sum: u64 = tiers.iter().map(|t| t.power_weight() as u64).sum();
    Ok((sum / tiers.len() as u64) as u32)
}

/// 画质估算（判据「性能/功耗权衡表」第二维）。
pub fn estimated_quality_permille(tiers: &[NodeTier]) -> Result<u32, u16> {
    if tiers.is_empty() {
        return Err(SEG_BASE + 4);
    }
    let sum: u64 = tiers.iter().map(|t| t.quality_weight() as u64).sum();
    Ok((sum / tiers.len() as u64) as u32)
}

/// 权衡表行（判据「性能/功耗权衡表」一等公民）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TradeoffRow {
    /// 档位。
    pub level: SaveLevel,
    /// 相对功耗（千分比）。
    pub power_permille: u32,
    /// 相对画质（千分比）。
    pub quality_permille: u32,
}

impl TradeoffRow {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.power_permille > 1000 || self.quality_permille > 1000 {
            return Err(SEG_BASE + 5);
        }
        Ok(())
    }
}

/// 权衡表构建与单调性校验（判据「性能/功耗权衡表」主判据）：档位
/// 升高（Off→Light→Aggressive）功耗单调不增、画质单调不增。
pub fn tradeoff_table_monotonic(rows: &[TradeoffRow]) -> Result<(), u16> {
    if rows.len() < 3 {
        return Err(SEG_BASE + 6);
    }
    for (i, r) in rows.iter().enumerate() {
        r.validate()?;
        if r.level.ordinal() as usize != i {
            return Err(SEG_BASE + 7);
        }
    }
    for w in rows.windows(2) {
        if w[1].power_permille > w[0].power_permille
            || w[1].quality_permille > w[0].quality_permille
        {
            return Err(SEG_BASE + 8);
        }
    }
    Ok(())
}

/// 帧率上限联动（判据「帧率联动」主判据）：省电模式下图执行频率
/// 被上限约束——请求帧率超上限时实际帧率 = 上限；未超则放行。
pub fn fps_cap_link(requested_fps_cent: u32) -> Result<(u32, bool), u16> {
    if requested_fps_cent == 0 {
        return Err(SEG_BASE + 9);
    }
    if requested_fps_cent > POWERSAVE_FPS_CAP_CENT {
        Ok((POWERSAVE_FPS_CAP_CENT, true))
    } else {
        Ok((requested_fps_cent, false))
    }
}

/// 功耗闭环（判据「功耗闭环」主判据）：窗口内实测功耗 → 自适应
/// 调整档位。判定：平均功耗仍高于目标 → 升档（更省）；低于目标
/// 过多（< 目标一半）→ 降档（还画质）。
pub fn power_feedback_loop(
    observed_permille: &[u32],
    target_permille: u32,
    current: SaveLevel,
) -> Result<SaveLevel, u16> {
    if observed_permille.len() < POWER_LOOP_WINDOW || target_permille == 0 {
        return Err(SEG_BASE + 10);
    }
    let avg: u64 = observed_permille.iter().map(|&p| p as u64).sum::<u64>() / observed_permille.len() as u64;
    if avg > target_permille as u64 {
        return Ok(match current {
            SaveLevel::Off => SaveLevel::Light,
            SaveLevel::Light => SaveLevel::Aggressive,
            SaveLevel::Aggressive => SaveLevel::Aggressive,
        });
    }
    if avg * 2 < target_permille as u64 {
        return Ok(match current {
            SaveLevel::Aggressive => SaveLevel::Light,
            SaveLevel::Light => SaveLevel::Off,
            SaveLevel::Off => SaveLevel::Off,
        });
    }
    Ok(current)
}

/// 省电达标（判据「功耗闭环」验收面）：实测功耗相对基准的降幅
/// ≥ POWER_SAVING_MIN_PERMILLE。
pub fn saving_ok(base_permille: u32, observed_permille: u32) -> Result<bool, u16> {
    if base_permille == 0 || observed_permille > base_permille {
        return Err(SEG_BASE + 11);
    }
    Ok(base_permille - observed_permille >= POWER_SAVING_MIN_PERMILLE)
}

/// 用户配置覆盖（判据「用户可配」语义面）：用户显式档位覆盖闭环
/// 裁决——Some(档位) 生效，None 跟随闭环。
pub fn user_save_override(auto: SaveLevel, manual: Option<SaveLevel>) -> SaveLevel {
    manual.unwrap_or(auto)
}

/// 变换账指纹（可审计面）。
pub fn transform_fp(nodes: &[PowerNode], tiers: &[NodeTier]) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for (n, t) in nodes.iter().zip(tiers.iter()) {
        acc = acc.wrapping_mul(0x100000001b3) ^ n.node as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ n.base.ordinal() as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ t.ordinal() as u64;
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    fn nodes() -> Vec<PowerNode> {
        vec![
            PowerNode { node: 1, base: NodeTier::Full, critical: true },
            PowerNode { node: 2, base: NodeTier::Full, critical: false },
            PowerNode { node: 3, base: NodeTier::HalfRes, critical: false },
        ]
    }
    #[test]
    fn transform_respects_criticality() {
        let ns = nodes();
        // Off：原样。
        let t0 = transform_for_power(&ns, SaveLevel::Off).unwrap();
        assert_eq!(t0, vec![NodeTier::Full, NodeTier::Full, NodeTier::HalfRes]);
        // Light：非关键 Full → HalfRes。
        let t1 = transform_for_power(&ns, SaveLevel::Light).unwrap();
        assert_eq!(t1, vec![NodeTier::Full, NodeTier::HalfRes, NodeTier::HalfRes]);
        // Aggressive：关键 → HalfRes，非关键 → Skipped。
        let t2 = transform_for_power(&ns, SaveLevel::Aggressive).unwrap();
        assert_eq!(t2, vec![NodeTier::HalfRes, NodeTier::Skipped, NodeTier::Skipped]);
        assert!(transform_fp(&ns, &t2) != 0);
    }
    #[test]
    fn power_quality_estimates() {
        let ns = nodes();
        let t2 = transform_for_power(&ns, SaveLevel::Aggressive).unwrap();
        // (550+0+0)/3 = 183。
        assert_eq!(estimated_power_permille(&t2).unwrap(), 183);
        // (500+0+0)/3 = 166。
        assert_eq!(estimated_quality_permille(&t2).unwrap(), 166);
        let t0 = transform_for_power(&ns, SaveLevel::Off).unwrap();
        assert_eq!(estimated_power_permille(&t0).unwrap(), 850); // (1000+1000+550)/3。
    }
    #[test]
    fn tradeoff_monotonicity() {
        let rows = vec![
            TradeoffRow { level: SaveLevel::Off, power_permille: 1000, quality_permille: 1000 },
            TradeoffRow { level: SaveLevel::Light, power_permille: 800, quality_permille: 850 },
            TradeoffRow { level: SaveLevel::Aggressive, power_permille: 400, quality_permille: 500 },
        ];
        assert!(tradeoff_table_monotonic(&rows).is_ok());
        // 违反单调 → 拒绝。
        let bad = vec![
            TradeoffRow { level: SaveLevel::Off, power_permille: 500, quality_permille: 1000 },
            TradeoffRow { level: SaveLevel::Light, power_permille: 800, quality_permille: 900 },
            TradeoffRow { level: SaveLevel::Aggressive, power_permille: 400, quality_permille: 500 },
        ];
        assert!(tradeoff_table_monotonic(&bad).is_err());
    }
    #[test]
    fn fps_cap_engages() {
        assert_eq!(fps_cap_link(6000).unwrap(), (3000, true));
        assert_eq!(fps_cap_link(2400).unwrap(), (2400, false));
        assert!(fps_cap_link(0).is_err());
    }
    #[test]
    fn power_loop_adapts() {
        let obs_high: Vec<u32> = vec![900; POWER_LOOP_WINDOW];
        assert_eq!(power_feedback_loop(&obs_high, 700, SaveLevel::Light).unwrap(), SaveLevel::Aggressive);
        let obs_low: Vec<u32> = vec![200; POWER_LOOP_WINDOW];
        assert_eq!(power_feedback_loop(&obs_low, 700, SaveLevel::Light).unwrap(), SaveLevel::Off);
        let obs_ok: Vec<u32> = vec![700; POWER_LOOP_WINDOW];
        assert_eq!(power_feedback_loop(&obs_ok, 700, SaveLevel::Light).unwrap(), SaveLevel::Light);
        // 窗口不足拒绝。
        assert!(power_feedback_loop(&[900; 4], 700, SaveLevel::Off).is_err());
    }
    #[test]
    fn saving_gate_and_override() {
        assert!(saving_ok(1000, 750).unwrap());
        assert!(!saving_ok(1000, 900).unwrap());
        assert_eq!(user_save_override(SaveLevel::Aggressive, Some(SaveLevel::Off)), SaveLevel::Off);
        assert_eq!(user_save_override(SaveLevel::Aggressive, None), SaveLevel::Aggressive);
    }
}
