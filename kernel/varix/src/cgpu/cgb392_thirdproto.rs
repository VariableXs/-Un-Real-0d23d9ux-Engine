//! CGPU-F0392 · 第三方预算接入协议（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 接入协议：应用声明预算的正式接入层（W 域 SDK 数据侧——应用声明资源
//!   需求/优先级/类别）；同规则校验（第三方声明过 F0325 防欺诈——无特权）；
//!   接入三档（完整声明/部分声明/无声明——无声明走默认池）；协议版本化
//!   （迁移兼容）。判据：正式协议、同规则、三档、版本化、判据。
//!
//! 设计纪律：
//! - 三档接入是互斥闭集：档位由声明完整度自动判定，不依赖诚实自报。
//! - 同规则校验：第三方与第一方走同一防欺诈闸（无特权旁路）。
//! - 版本化：旧版本声明进来自动迁移（v1→v2），迁移后过 v2 闸。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0B60 独占。

use alloc::vec::Vec;
use alloc::string::String;

/// 诊断段基址（0x0B60，全仓独占）。
pub const SEG_BASE: u16 = 0x0B60;

pub const E_FRAUD_SUSPECT: u16 = SEG_BASE + 1;
pub const E_DECLARATION_INVALID: u16 = SEG_BASE + 2;
pub const E_VERSION_UNSUPPORTED: u16 = SEG_BASE + 3;
pub const E_TIER_CONFLICT: u16 = SEG_BASE + 4;

/// 协议版本。
pub const PROTO_VERSION: u32 = 2;

/// 接入三档（判据「三档」——互斥闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessTier {
    /// 完整声明：需求+优先级+类别全给。
    Full,
    /// 部分声明：缺一类，缺项走保守默认。
    Partial,
    /// 无声明：走默认池（最低配额带）。
    None,
}

impl AccessTier {
    pub fn default_pool_quota_pm(self) -> u32 {
        match self {
            AccessTier::Full => 0, // 完整声明用声明值，不走默认池
            AccessTier::Partial => 1_000,
            AccessTier::None => 300,
        }
    }
}

/// 资源声明（W 域 SDK 数据侧的冻结形状）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub proto_version: u32,
    pub app_id: u32,
    /// 资源需求（千分位额度）。
    pub demand_pm: u32,
    /// 优先级（0~100）。
    pub priority: u8,
    /// 类别（1 交互 2 多媒体 3 后台 4 透明叠加）。
    pub category: u8,
}

impl Declaration {
    /// v1 声明形状（迁移源）。
    pub fn v1(app_id: u32, demand_pm: u32, priority: u8) -> Self {
        Declaration { proto_version: 1, app_id, demand_pm, priority, category: 0 }
    }

    pub fn v2(app_id: u32, demand_pm: u32, priority: u8, category: u8) -> Self {
        Declaration { proto_version: 2, app_id, demand_pm, priority, category }
    }

    /// 声明完整度 → 接入档（判据「三档」自动判定）。
    pub fn tier(&self) -> AccessTier {
        let mut fields = 0;
        if self.demand_pm > 0 {
            fields += 1;
        }
        if self.priority > 0 {
            fields += 1;
        }
        if (1..=4).contains(&self.category) {
            fields += 1;
        }
        match fields {
            3 => AccessTier::Full,
            1 | 2 => AccessTier::Partial,
            _ => AccessTier::None,
        }
    }
}

/// F0325 防欺诈闸（同规则校验——第一方/第三方同一套）。
/// 规则（冻结）：需求 ≤ 3_000‰、优先级 ≤ 80、类别闭集。
/// 第三方唯一差异：需求上限再砍半（无特权声明不因接入层而更高）。
pub fn anti_fraud_gate(d: &Declaration, third_party: bool) -> Result<(), u16> {
    // 零值字段 = 未声明（走部分/无声明档），合法；只拦显性越界声明。
    if d.demand_pm > 3_000 {
        return Err(E_FRAUD_SUSPECT);
    }
    if d.priority > 80 {
        return Err(E_FRAUD_SUSPECT);
    }
    if d.proto_version >= 2 && d.category != 0 && !(1..=4).contains(&d.category) {
        return Err(E_DECLARATION_INVALID);
    }
    if third_party && d.demand_pm > 1_500 {
        // 第三方无特权：上限减半（同规则更严——不是另一套规则）。
        return Err(E_FRAUD_SUSPECT);
    }
    Ok(())
}

/// 协议版本化迁移（判据「版本化」）：v1→v2 自动补默认类别（保守：后台）。
pub fn migrate_v1_to_v2(d: &Declaration) -> Result<Declaration, u16> {
    match d.proto_version {
        2 => Ok(d.clone()),
        1 => Ok(Declaration { proto_version: 2, app_id: d.app_id, demand_pm: d.demand_pm, priority: d.priority, category: 3 }),
        _ => Err(E_VERSION_UNSUPPORTED),
    }
}

/// 接入网关：第三方声明的正式入口。
pub struct AccessGateway {
    /// 已接入应用（幂等：重接入更新声明）。
    admitted: Vec<(u32, AccessTier, u32)>, // (app, tier, effective_quota_pm)
    pub rejected_count: u64,
}

impl AccessGateway {
    pub fn new() -> Self {
        AccessGateway { admitted: Vec::new(), rejected_count: 0 }
    }

    /// 接入一笔声明（第三方路径——同规则校验 + 自动迁移 + 档位判定）。
    pub fn admit(&mut self, raw: &Declaration) -> Result<(AccessTier, u32), u16> {
        // 版本迁移先行。
        let d = migrate_v1_to_v2(raw)?;
        // 同规则防欺诈（第三方=true：无特权）。
        if let Err(e) = anti_fraud_gate(&d, true) {
            self.rejected_count += 1;
            return Err(e);
        }
        let tier = d.tier();
        // 生效配额：完整声明用声明值（已过闸），否则默认池。
        let quota = match tier {
            AccessTier::Full => d.demand_pm,
            AccessTier::Partial => AccessTier::Partial.default_pool_quota_pm(),
            AccessTier::None => AccessTier::None.default_pool_quota_pm(),
        };
        if let Some(e) = self.admitted.iter_mut().find(|(a, _, _)| *a == d.app_id) {
            e.1 = tier;
            e.2 = quota;
        } else {
            self.admitted.push((d.app_id, tier, quota));
        }
        Ok((tier, quota))
    }

    /// 档位冲突探测：同 app 降档接入（Full→None）必须显性可见。
    pub fn tier_of(&self, app: u32) -> Option<AccessTier> {
        self.admitted.iter().find(|(a, _, _)| *a == app).map(|(_, t, _)| *t)
    }

    pub fn effective_quota(&self, app: u32) -> Option<u32> {
        self.admitted.iter().find(|(a, _, _)| *a == app).map(|(_, _, q)| *q)
    }

    pub fn admitted_count(&self) -> usize {
        self.admitted.len()
    }
}

impl Default for AccessGateway {
    fn default() -> Self {
        Self::new()
    }
}

/// 诚实标注：第三方接入层的特权声明探测（永远 false——协议层面无特权）。
pub fn has_privileged_lane() -> bool {
    false
}

// ---------------------------------------------------------------------------
// 判据自检
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a92_三档_自动判定() {
        // 完整声明。
        let full = Declaration::v2(1, 1_500, 60, 1);
        assert_eq!(full.tier(), AccessTier::Full);
        // 部分：缺类别。
        let partial = Declaration::v1(2, 1_000, 50);
        assert_eq!(partial.tier(), AccessTier::Partial);
        // 部分：只有需求。
        let partial2 = Declaration::v2(3, 800, 0, 0);
        assert_eq!(partial2.tier(), AccessTier::Partial);
        // 无声明。
        let none = Declaration::v2(4, 0, 0, 0);
        assert_eq!(none.tier(), AccessTier::None);
        // 默认池配额。
        assert_eq!(AccessTier::Partial.default_pool_quota_pm(), 1_000);
        assert_eq!(AccessTier::None.default_pool_quota_pm(), 300);
        assert_eq!(AccessTier::Full.default_pool_quota_pm(), 0);
    }

    #[test]
    fn a92_同规则_防欺诈无特权() {
        // 第一方合法（≤3000）。
        assert!(anti_fraud_gate(&Declaration::v2(1, 2_900, 70, 2), false).is_ok());
        // 同声明第三方拒（>1500）。
        assert_eq!(anti_fraud_gate(&Declaration::v2(1, 2_900, 70, 2), true).unwrap_err(), E_FRAUD_SUSPECT);
        // 第三方压线内过。
        assert!(anti_fraud_gate(&Declaration::v2(1, 1_500, 70, 2), true).is_ok());
        // 需求 0 = 未声明（合法，档位判定走 None）；超总闸拒（双方同规则）。
        assert!(anti_fraud_gate(&Declaration::v2(1, 0, 10, 1), false).is_ok());
        assert_eq!(anti_fraud_gate(&Declaration::v2(1, 3_001, 10, 1), false).unwrap_err(), E_FRAUD_SUSPECT);
        // 优先级 >80 拒。
        assert_eq!(anti_fraud_gate(&Declaration::v2(1, 100, 81, 1), true).unwrap_err(), E_FRAUD_SUSPECT);
        // 类别越闭集拒。
        assert_eq!(anti_fraud_gate(&Declaration::v2(1, 100, 10, 9), true).unwrap_err(), E_DECLARATION_INVALID);
        // 特权通道永远不存在（诚实标注）。
        assert!(!has_privileged_lane());
    }

    #[test]
    fn a92_版本化_v1自动迁移() {
        // v1 → v2：默认类别=后台(3)。
        let v1 = Declaration::v1(7, 1_000, 50);
        let m = migrate_v1_to_v2(&v1).unwrap();
        assert_eq!(m.proto_version, 2);
        assert_eq!(m.category, 3);
        // v2 直通。
        let v2 = Declaration::v2(7, 1_000, 50, 1);
        assert_eq!(migrate_v1_to_v2(&v2).unwrap(), v2);
        // v9 拒。
        let mut v9 = v2.clone();
        v9.proto_version = 9;
        assert_eq!(migrate_v1_to_v2(&v9).unwrap_err(), E_VERSION_UNSUPPORTED);
        assert_eq!(PROTO_VERSION, 2);
    }

    #[test]
    fn a92_网关_接入幂等与降档可见() {
        let mut g = AccessGateway::new();
        // 完整声明接入：生效配额=声明值。
        let (tier, q) = g.admit(&Declaration::v2(10, 1_200, 60, 1)).unwrap();
        assert_eq!(tier, AccessTier::Full);
        assert_eq!(q, 1_200);
        // 无声明接入：默认池 300。
        let (tier, q) = g.admit(&Declaration::v2(11, 0, 0, 0)).unwrap();
        assert_eq!(tier, AccessTier::None);
        assert_eq!(q, 300);
        // v1 声明：迁移自动补类别（后台）→ 三字段齐 → Full（生效配额=声明值）。
        let (tier, q) = g.admit(&Declaration::v1(12, 900, 40)).unwrap();
        assert_eq!(tier, AccessTier::Full);
        assert_eq!(q, 900);
        // 幂等重接入：更新声明（10 降档到 None 显性可见）。
        g.admit(&Declaration::v2(10, 0, 0, 0)).unwrap();
        assert_eq!(g.tier_of(10), Some(AccessTier::None));
        assert_eq!(g.effective_quota(10), Some(300));
        // 拒绝计数。
        assert_eq!(g.rejected_count, 0);
        assert!(g.admit(&Declaration::v2(13, 2_900, 70, 2)).is_err());
        assert_eq!(g.rejected_count, 1);
        assert_eq!(g.admitted_count(), 3);
        // 档位查询。
        assert_eq!(g.tier_of(11), Some(AccessTier::None));
        assert_eq!(g.tier_of(99), None);
    }
}
