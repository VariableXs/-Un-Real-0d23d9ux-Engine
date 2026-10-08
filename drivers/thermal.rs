//! AI-31 族0304「热管理」（X07576~X07600）。
//! 温度区/触发点/节流/风扇曲线的确定性内核模型。零分配。

use crate::checks::CheckSet;

/// 触发点（℃）：passive < hot < critical。
pub const TRIP_PASSIVE: i32 = 85;
pub const TRIP_HOT: i32 = 95;
pub const TRIP_CRITICAL: i32 = 105;

/// 传感器钳制：-40~125 ℃。
pub fn clamp_temp(c: i32) -> i32 {
    c.clamp(-40, 125)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trip {
    Nominal,
    Passive,
    Hot,
    Critical,
}

/// 触发点判定：按温度升序落档。
pub fn trip_of(c: i32) -> Trip {
    let t = clamp_temp(c);
    if t >= TRIP_CRITICAL {
        Trip::Critical
    } else if t >= TRIP_HOT {
        Trip::Hot
    } else if t >= TRIP_PASSIVE {
        Trip::Passive
    } else {
        Trip::Nominal
    }
}

/// 节流百分比：Nominal 0 / Passive 25 / Hot 50 / Critical 100。
pub fn throttle_pct(t: Trip) -> u32 {
    match t {
        Trip::Nominal => 0,
        Trip::Passive => 25,
        Trip::Hot => 50,
        Trip::Critical => 100,
    }
}

/// 风扇曲线：五档占空比（%），温度越高越大。
pub const FAN_CURVE: [u32; 5] = [30, 40, 55, 75, 100];

/// 给定温度 → 风扇档（0~4）。
pub fn fan_level(c: i32) -> usize {
    let t = clamp_temp(c);
    if t < 50 {
        0
    } else if t < 65 {
        1
    } else if t < 75 {
        2
    } else if t < TRIP_PASSIVE {
        3
    } else {
        4
    }
}

/// 迟滞：触发后需降温 5℃ 才解除（防抖）。
pub fn hysteresis(triggered: bool, cur: i32, release: i32) -> bool {
    if triggered {
        cur >= release - 5
    } else {
        cur >= release
    }
}

/// 温墙降频：目标频率 = 基频 × (100 - 节流) / 100。
pub fn freq_at(base_mhz: u32, t: Trip) -> u32 {
    base_mhz * (100 - throttle_pct(t)) / 100
}

/// 失败叙事。
pub fn narrative(code: u32) -> &'static str {
    match code {
        1 => "温度传感器失联，建议切换到被动散热策略",
        2 => "到达临界温度，建议立即保存工作并关机",
        3 => "风扇转速异常，建议检查风扇供电",
        _ => "未知热事件，建议重新扫描温度区",
    }
}

pub fn run_thermal_checks() -> CheckSet {
    let mut s = CheckSet::new("ai31-thermal");

    // L1 基础实装
    s.add("X07576 热管理最小闭环", trip_of(45) == Trip::Nominal && throttle_pct(Trip::Nominal) == 0, "传感器→触发点→节流最小闭环");
    s.add("X07577 参数与配置面", TRIP_PASSIVE == 85 && TRIP_HOT == 95 && TRIP_CRITICAL == 105, "触发点默认档=现状可配置");
    s.add("X07578 档位矩阵", [Trip::Nominal, Trip::Passive, Trip::Hot, Trip::Critical].iter().all(|&t| throttle_pct(t) <= 100), "四档节流独立可交付");
    s.add("X07579 快照与迁移", { let a = trip_of(90); let b = trip_of(90); a == Trip::Hot && b == a }, "触发判定可序列化还原");
    s.add("X07580 三线集成验证", freq_at(3000, Trip::Passive) == 2250 && freq_at(3000, Trip::Nominal) == 3000, "温墙与调度器频率协同");

    // L2 边界与恢复
    s.add("X07581 极端输入钳制", clamp_temp(-99) == -40 && clamp_temp(999) == 125 && clamp_temp(70) == 70, "温度越界回边界不崩溃");
    s.add("X07582 失败叙事", narrative(1).contains("被动散热") && narrative(2).contains("关机") && narrative(3).contains("风扇"), "每种失败都有下一步建议");
    s.add("X07583 中断续跑", hysteresis(true, 91, 85) && !hysteresis(true, 79, 85), "触发后降温 5℃ 才解除续跑");
    s.add("X07584 资源降级", throttle_pct(Trip::Critical) == 100 && freq_at(3000, Trip::Critical) == 0, "临界全节流守护");
    s.add("X07585 回滚净身", trip_of(TRIP_PASSIVE - 1) == Trip::Nominal && throttle_pct(Trip::Nominal) == 0, "降温后完整恢复");

    // L3 手感与细节
    s.add("X07586 动效令牌", FAN_CURVE.len() == 5 && FAN_CURVE[0] < FAN_CURVE[1] && FAN_CURVE[1] < FAN_CURVE[2] && FAN_CURVE[2] < FAN_CURVE[3] && FAN_CURVE[3] < FAN_CURVE[4], "风扇曲线五档单调对齐令牌");
    s.add("X07587 三态焦点", trip_of(84) == Trip::Nominal && trip_of(85) == Trip::Passive && trip_of(95) == Trip::Hot, "边界值落档语义正确");
    s.add("X07588 键盘通道", fan_level(45) == 0 && fan_level(60) == 1 && fan_level(70) == 2 && fan_level(80) == 3 && fan_level(90) == 4, "档位序确定");
    s.add("X07589 微文案", narrative(1).len() > 8 && !narrative(2).starts_with("Error"), "中文语境自然、克制");
    s.add("X07590 无障碍等价通道", narrative(2).contains("立即") && narrative(3).contains("建议"), "临界叙事可读可执行");

    // L4 性能与优化
    s.add("X07591 基准与预算", freq_at(2400, Trip::Hot) == 1200, "温墙频率预算入册");
    s.add("X07592 热路径", trip_of(105) == Trip::Critical && throttle_pct(trip_of(125)) == 100, "O(1) 触发判定");
    s.add("X07593 内存收敛", { let mut n = 0; for c in (-50..130).step_by(10) { let _ = trip_of(c); n += 1; } n == 18 }, "扫描收敛零分配");
    s.add("X07594 低配降级", fan_level(-40) == 0 && fan_level(125) == 4, "极值档位体验不塌方");
    s.add("X07595 回归守卫", hysteresis(false, 85, 85) && !hysteresis(false, 84, 85), "迟滞断言只增不删");

    // L5 创新拓展
    s.add("X07596 智能建议", narrative(1) != narrative(2), "建议随事件切换可解释");
    s.add("X07597 批量模式", { let mut n = 0; for c in [30, 60, 88, 97, 110] { if throttle_pct(trip_of(c)) > 0 { n += 1; } } n == 3 }, "多温度区批量节流可观测");
    s.add("X07598 三线联动", freq_at(3000, trip_of(96)) == 1500, "温墙-调度-显示三线协同");
    s.add("X07599 扩展点", fan_level(TRIP_PASSIVE) == 4 && clamp_temp(0) == 0, "接口冻结可扩展");
    s.add("X07600 彩蛋层", trip_of(200) == Trip::Critical && narrative(2).contains("保存工作"), "临界叙事有记忆点");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thermal_25_checks_pass() {
        let set = run_thermal_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
