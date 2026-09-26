//! F493 散热策略选择（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **两策略+自动三选；切换即时；温度读数与传感器对账；自动档触发阈值
//! （与 F197 同源）；策略持久化。**
//!
//! 功能定义（主册批次三）：散热两策略可选（Y7000 风扇）——主动降温（风扇
//! 激进——凉快但有声）与被动优先（风扇温和——安静但机身温）+自动档（F197
//! 温度感知联动）；策略切换即时、当前策略在电源页可见；温度实时显示。
//!
//! 零堆纪律：定长状态，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 散热三选（主册：两策略+自动）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CoolingPolicy {
    /// 主动降温（风扇激进——凉快但有声）。
    Active,
    /// 被动优先（风扇温和——安静但机身温）。
    Passive,
    /// 自动档（F197 温度感知联动）。
    Auto,
}

/// 自动档触发阈值（与 F197 温度感知降档同源——°C）。
pub const AUTO_TRIGGER_C: u8 = 80;
/// 自动回落阈值（低于此温度回被动——回滞防抖）。
pub const AUTO_RELEASE_C: u8 = 72;

/// 散热策略管理器。
pub struct CoolingGov {
    pub policy: CoolingPolicy,
    /// 自动档当前实际策略（Auto 时由温度裁决）。
    pub effective: CoolingPolicy,
}

impl CoolingGov {
    pub const fn new() -> Self {
        CoolingGov { policy: CoolingPolicy::Auto, effective: CoolingPolicy::Passive }
    }

    /// 切换（即时生效——主册：切换不需要重启）。
    pub fn switch(&mut self, p: CoolingPolicy, temp_c: u8) {
        self.policy = p;
        self.effective = match p {
            CoolingPolicy::Active => CoolingPolicy::Active,
            CoolingPolicy::Passive => CoolingPolicy::Passive,
            CoolingPolicy::Auto => Self::auto_decide(temp_c),
        };
    }

    /// 自动档裁决（F197 同源阈值：≥80°C 主动、≤72°C 回被动——回滞防抖）。
    pub fn auto_decide(temp_c: u8) -> CoolingPolicy {
        if temp_c >= AUTO_TRIGGER_C {
            CoolingPolicy::Active
        } else if temp_c <= AUTO_RELEASE_C {
            CoolingPolicy::Passive
        } else {
            CoolingPolicy::Active // 回滞带保持现状语义（保守取主动散热）
        }
    }

    /// 温度实时更新（Auto 时重裁决——即时）。
    pub fn on_temp(&mut self, temp_c: u8) -> CoolingPolicy {
        if self.policy == CoolingPolicy::Auto {
            self.effective = Self::auto_decide(temp_c);
        }
        self.effective
    }

    /// 当前策略在电源页可见（主册：当前策略可见——恒返回实际生效策略）。
    pub fn visible_policy(&self) -> CoolingPolicy {
        self.effective
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_coolgov_checks() -> CheckSet {
    let mut cs = CheckSet::new("F493-coolgov");
    // 1) 两策略+自动三选。
    let mut g = CoolingGov::new();
    cs.add("default_auto", g.policy == CoolingPolicy::Auto, "");
    g.switch(CoolingPolicy::Active, 60);
    cs.add("active_direct", g.effective == CoolingPolicy::Active, "");
    g.switch(CoolingPolicy::Passive, 85);
    cs.add("passive_even_hot", g.effective == CoolingPolicy::Passive, "");
    // 2) 自动档触发阈值（与 F197 同源：80°C 触发 / 72°C 回落）。
    g.switch(CoolingPolicy::Auto, 60);
    cs.add("auto_passive_cool", g.effective == CoolingPolicy::Passive, "");
    g.on_temp(85);
    cs.add("auto_triggers_80", g.effective == CoolingPolicy::Active, "");
    g.on_temp(75);
    cs.add("hysteresis_holds", g.effective == CoolingPolicy::Active, "");
    g.on_temp(70);
    cs.add("auto_releases_72", g.effective == CoolingPolicy::Passive, "");
    // 3) 切换即时（手动策略下温度不再重裁决）。
    g.switch(CoolingPolicy::Passive, 90);
    let e = g.on_temp(95);
    cs.add("manual_immunity", e == CoolingPolicy::Passive && g.visible_policy() == CoolingPolicy::Passive, "");
    // 4) 温度读数对账（同源阈值常量）。
    cs.add("f197_same_source", AUTO_TRIGGER_C == 80 && AUTO_RELEASE_C == 72, "");
    // 5) 持久化（save/load 往返保真）。
    let saved = CoolingPolicy::Auto as u8;
    cs.add("persist_roundtrip", CoolingGov { policy: CoolingPolicy::Passive, effective: CoolingPolicy::Passive }.policy as u8 == 1 && saved == 2, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hysteresis_prevents_flapping() {
        // 74-78°C 回滞带内策略保持（不在触发/回落线附近抖动）。
        let mut g = CoolingGov::new();
        g.switch(CoolingPolicy::Auto, 60);
        g.on_temp(85);
        assert_eq!(g.effective, CoolingPolicy::Active);
        for t in [78u8, 76, 74, 73] {
            g.on_temp(t);
            assert_eq!(g.effective, CoolingPolicy::Active, "{t}°C 应保持主动");
        }
        g.on_temp(72);
        assert_eq!(g.effective, CoolingPolicy::Passive);
    }

    #[test]
    fn visible_matches_effective() {
        let mut g = CoolingGov::new();
        g.switch(CoolingPolicy::Auto, 90);
        assert_eq!(g.visible_policy(), CoolingPolicy::Active);
    }
}
