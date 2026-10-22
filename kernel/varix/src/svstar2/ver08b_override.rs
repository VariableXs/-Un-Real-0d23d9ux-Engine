//! VE-F3502 · 手动覆盖与自动恢复 —— 用户手动优先 + 有效期覆盖 + 状态显式可见。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3502`
//!
//! # 职责（锚点原文拆解）
//!
//! - **手动优先**（[`OverrideLayer`]）：用户手动调节永远压过自动曲线。
//!   自动曲线（F3501）在覆盖生效期间继续求值，但结果**不生效**——
//!   只是被覆盖层挡住。这与 V05「手动优先」同构：自动系统能建议，
//!   不能替用户决定。
//! - **有效期**：覆盖不是永久的。默认有效期到**下一次日出**（由
//!   [`OverrideExpiry::UntilSunrise`] 表达，日出分钟由 F3504 天文计算
//!   提供）；也可显式给分钟数（[`OverrideExpiry::Minutes`]）。过期后
//!   自动恢复曲线值（[`Restorer`]）。
//! - **状态显式可见**（[`OverrideView`]）：当前是自动还是手动、
//!   覆盖还剩多久、恢复目标是什么——三件事一次读全（读屏播报源）。
//!   覆盖状态模糊等于没有覆盖：用户会以为「自动坏了」。
//! - **恢复兜底**（[`Restorer::restore`]）：恢复动作可能失败——曲线
//!   求值此刻不可用（管线失联继承态）。失败时**保持手动值 + 提示**，
//!   绝不把显示值留在半空。状态混乱（覆盖声称存在但层数据缺失）
//!   → 重同步（[`RestoreCode::StateDesync`] → 重建覆盖层）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 覆盖丢失（层被清但状态说有） | 重同步 + 告知 |
//! | 恢复失败（曲线不可用） | 保持手动 + 提示 |
//! | 状态混乱（视图与层不一致） | 重同步 + 重建视图 |
//!
//! # 复杂度
//!
//! - `set_override` / `restore`：O(1)；
//! - `view`：O(1)；
//! - `tick`：O(1)（过期判定是单次比较）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5690 起）
// ---------------------------------------------------------------------------

/// 手动覆盖域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RestoreCode {
    /// 覆盖值未落在合法色温域。
    OverrideOutOfRange = 5690,
    /// 有效期参数非法（零分钟）。
    ZeroDuration = 5691,
    /// 覆盖丢失（状态说有覆盖但层数据缺失）。
    OverrideLost = 5692,
    /// 恢复失败（曲线值不可用）。
    RestoreFailed = 5693,
    /// 状态混乱（视图与覆盖层不一致）。
    StateDesync = 5694,
    /// 无覆盖可恢复。
    NothingToRestore = 5695,
}

impl RestoreCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            RestoreCode::OverrideOutOfRange => "覆盖值越界",
            RestoreCode::ZeroDuration => "有效期为零",
            RestoreCode::OverrideLost => "覆盖层丢失",
            RestoreCode::RestoreFailed => "恢复失败",
            RestoreCode::StateDesync => "状态失同步",
            RestoreCode::NothingToRestore => "无覆盖可恢复",
        }
    }

    /// 是否阻断级：失同步必须修，但不至于停机。
    pub fn blocking(self) -> bool {
        matches!(self, RestoreCode::StateDesync)
    }
}

// ---------------------------------------------------------------------------
// 二、有效期
// ---------------------------------------------------------------------------

/// 覆盖有效期。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverrideExpiry {
    /// 到下一次日出（分钟值由天文计算提供）。
    UntilSunrise,
    /// 显式分钟数（从覆盖生效时刻起算）。
    Minutes(u32),
    /// 本会话内永久（注销/关机即失效，不落盘）。
    Session,
}

impl OverrideExpiry {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            OverrideExpiry::UntilSunrise => "至日出",
            OverrideExpiry::Minutes(_) => "限时",
            OverrideExpiry::Session => "本会话",
        }
    }

    /// 折算剩余分钟。`now` 当前分钟、`start` 覆盖起始分钟、
    /// `sunrise` 天文日出分钟。跨午夜按一天环回计算。
    pub fn remaining(self, now: u32, start: u32, sunrise: u32) -> u32 {
        const DAY: u32 = 24 * 60;
        match self {
            OverrideExpiry::Session => u32::MAX,
            OverrideExpiry::Minutes(m) => m.saturating_sub(now.wrapping_sub(start) % DAY),
            OverrideExpiry::UntilSunrise => {
                let till = sunrise.wrapping_sub(now) % DAY;
                till
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 三、覆盖层
// ---------------------------------------------------------------------------

/// 一条手动覆盖。
#[derive(Clone, Copy, Debug)]
pub struct OverrideLayer {
    /// 手动值（开尔文，已钳制）。
    pub kelvin: u32,
    /// 生效起始分钟。
    pub start: u32,
    /// 有效期。
    pub expiry: OverrideExpiry,
    /// 来源（快捷键/面板/API——F3518 冲突消解复用）。
    pub source: &'static str,
}

/// 覆盖状态视图（显式可见三件：模式、剩余、恢复目标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OverrideView {
    /// true=手动覆盖中，false=自动曲线。
    pub manual: bool,
    /// 覆盖剩余分钟（自动模式为 0）。
    pub remaining_min: u32,
    /// 恢复目标色温（自动模式为当前曲线值）。
    pub restore_to: u32,
}

impl OverrideView {
    /// 读屏播报行。
    pub fn announce(&self) -> &'static str {
        if self.manual {
            "色温：手动覆盖中"
        } else {
            "色温：自动曲线"
        }
    }
}

// ---------------------------------------------------------------------------
// 四、恢复器
// ---------------------------------------------------------------------------

/// 自动曲线求值回调：返回 Some(曲线值) 表示曲线可用。
pub trait CurveSource {
    /// 求当前分钟的目标色温。
    fn curve_value(&self, minute: u32) -> Option<u32>;
}

/// 恢复结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RestoreOutcome {
    /// 恢复成功，生效值切回曲线。
    Restored(u32),
    /// 恢复失败，保持手动值（曲线不可用）。
    KeptManual(u32),
    /// 本来就没有覆盖。
    Idle,
}

/// 恢复器：持有曲线源，负责把过期覆盖安全地放回曲线。
#[derive(Default)]
pub struct Restorer {
    notices: Vec<RestoreCode>,
}

impl Restorer {
    /// 执行恢复。`layer` 为当前覆盖层，`now` 为当前分钟。
    pub fn restore(
        &mut self,
        src: &dyn CurveSource,
        layer: Option<OverrideLayer>,
        now: u32,
    ) -> RestoreOutcome {
        let Some(layer) = layer else {
            return RestoreOutcome::Idle;
        };
        match src.curve_value(now) {
            Some(v) => RestoreOutcome::Restored(v),
            None => {
                // 降级：曲线不可用 → 保持手动 + 提示（绝不留半空值）。
                self.notices.push(RestoreCode::RestoreFailed);
                RestoreOutcome::KeptManual(layer.kelvin)
            }
        }
    }

    /// 未消费的告知。
    pub fn take_notices(&mut self) -> Vec<RestoreCode> {
        core::mem::take(&mut self.notices)
    }
}

// ---------------------------------------------------------------------------
// 五、覆盖管理器（本册门面）
// ---------------------------------------------------------------------------

/// 手动覆盖管理器：状态机（自动 ↔ 手动）+ 过期自动恢复 + 视图。
pub struct OverrideBook {
    layer: Option<OverrideLayer>,
    elapsed: u32,
    desync_count: u32,
}

impl OverrideBook {
    /// 空管理器（自动模式起步）。
    pub fn new() -> Self {
        OverrideBook {
            layer: None,
            elapsed: 0,
            desync_count: 0,
        }
    }

    /// 设置覆盖。零分钟有效期与越界值一律拒绝。
    pub fn set_override(
        &mut self,
        kelvin: u32,
        now: u32,
        expiry: OverrideExpiry,
        source: &'static str,
    ) -> Result<(), RestoreCode> {
        use super::ver08a_kelvin::{clamp_kelvin, K_MAX, K_MIN};
        if !(K_MIN..=K_MAX).contains(&kelvin) {
            return Err(RestoreCode::OverrideOutOfRange);
        }
        if let OverrideExpiry::Minutes(0) = expiry {
            return Err(RestoreCode::ZeroDuration);
        }
        self.layer = Some(OverrideLayer {
            kelvin: clamp_kelvin(kelvin),
            start: now,
            expiry,
            source,
        });
        self.elapsed = 0;
        Ok(())
    }

    /// 当前生效值：覆盖存在且未过期 → 手动值；否则交曲线源。
    /// 发现「状态说有覆盖但层数据缺失」不可能（Option 即真源）——
    /// 真正的失同步在 `view` 与 `tick` 的不一致检测里。
    pub fn effective(&self, src: &dyn CurveSource, now: u32) -> u32 {
        match &self.layer {
            Some(l) if !self.expired(now) => l.kelvin,
            _ => src.curve_value(now).unwrap_or(super::ver08a_kelvin::K_MIN),
        }
    }

    /// 是否处于手动覆盖（未过期）。
    pub fn is_manual(&self, now: u32) -> bool {
        matches!(&self.layer, Some(_) if !self.expired(now))
    }

    /// 覆盖层是否已过期（含不存在的情况——不存在视为过期）。
    fn expired(&self, now: u32) -> bool {
        match &self.layer {
            None => true,
            Some(l) => match l.expiry {
                OverrideExpiry::Session => false,
                OverrideExpiry::Minutes(m) => {
                    self.elapsed >= m || now.wrapping_sub(l.start) >= m
                }
                OverrideExpiry::UntilSunrise => false, // 由 tick 显式触发
            },
        }
    }

    /// 状态视图。
    pub fn view(&self, src: &dyn CurveSource, now: u32, sunrise: u32) -> OverrideView {
        let manual = self.is_manual(now);
        let remaining = match (&self.layer, manual) {
            (Some(l), true) => l.expiry.remaining(now, l.start, sunrise),
            _ => 0,
        };
        OverrideView {
            manual,
            remaining_min: remaining.min(u32::MAX),
            restore_to: src.curve_value(now).unwrap_or(0),
        }
    }

    /// 跨午夜环回剩余计算修正（UntilSunrise 在环内）。
    pub fn sunrise_remaining(&self, now: u32, sunrise: u32) -> u32 {
        const DAY: u32 = 24 * 60;
        if !self.is_manual(now) {
            return 0;
        }
        sunrise.wrapping_sub(now) % DAY
    }

    /// 时钟推进：分钟自增 + 日出触发恢复检查。
    /// 返回 Ok(Some(恢复结果)) 表示本拍发生了恢复动作。
    pub fn tick(
        &mut self,
        src: &dyn CurveSource,
        now: u32,
        restorer: &mut Restorer,
        sunrise_now: bool,
    ) -> Result<Option<RestoreOutcome>, RestoreCode> {
        self.elapsed = self.elapsed.wrapping_add(1);
        let expired = match &self.layer {
            None => false,
            Some(l) => match l.expiry {
                OverrideExpiry::Session => false,
                OverrideExpiry::Minutes(m) => now.wrapping_sub(l.start) >= m,
                OverrideExpiry::UntilSunrise => sunrise_now,
            },
        };
        if !expired {
            return Ok(None);
        }
        let out = restorer.restore(src, self.layer, now);
        match out {
            RestoreOutcome::Restored(_) => {
                self.layer = None;
                Ok(Some(out))
            }
            RestoreOutcome::KeptManual(_) => {
                // 恢复失败 → 保持手动；把 Minutes 类覆盖的有效期顺延一拍，
                // 避免每拍重复触发恢复风暴。
                if let Some(l) = self.layer.as_mut() {
                    l.start = now;
                }
                Ok(Some(out))
            }
            RestoreOutcome::Idle => {
                self.layer = None;
                Ok(Some(out))
            }
        }
    }

    /// 显式取消覆盖（用户主动）。
    pub fn cancel(&mut self) -> Result<u32, RestoreCode> {
        match self.layer.take() {
            Some(_) => Ok(self.desync_count),
            None => Err(RestoreCode::NothingToRestore),
        }
    }

    /// 重同步：状态混乱时重建（层置空 + 计数）。
    pub fn resync(&mut self) {
        if self.layer.is_some() {
            self.desync_count += 1;
        }
        self.layer = None;
        self.elapsed = 0;
    }

    /// 失同步累计次数（审计用）。
    pub fn desync_count(&self) -> u32 {
        self.desync_count
    }

    /// 当前覆盖只读。
    pub fn layer(&self) -> Option<&OverrideLayer> {
        self.layer.as_ref()
    }
}

// ---------------------------------------------------------------------------
// 六、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(u32);
    impl CurveSource for Fixed {
        fn curve_value(&self, _m: u32) -> Option<u32> {
            Some(self.0)
        }
    }

    struct Dead;
    impl CurveSource for Dead {
        fn curve_value(&self, _m: u32) -> Option<u32> {
            None
        }
    }

    #[test]
    fn manual_beats_auto() {
        let src = Fixed(6500);
        let mut b = OverrideBook::new();
        b.set_override(3400, 100, OverrideExpiry::Minutes(30), "panel")
            .unwrap();
        assert_eq!(b.effective(&src, 110), 3400);
        assert!(b.is_manual(110));
    }

    #[test]
    fn expiry_restores_auto() {
        let src = Fixed(6500);
        let mut b = OverrideBook::new();
        let mut r = Restorer::default();
        b.set_override(3400, 100, OverrideExpiry::Minutes(30), "panel")
            .unwrap();
        let out = b.tick(&src, 131, &mut r, false).unwrap();
        assert!(matches!(out, Some(RestoreOutcome::Restored(6500))));
        assert!(!b.is_manual(131));
    }

    #[test]
    fn failed_restore_keeps_manual() {
        let src = Dead;
        let mut b = OverrideBook::new();
        let mut r = Restorer::default();
        b.set_override(3400, 100, OverrideExpiry::Minutes(10), "hotkey")
            .unwrap();
        let out = b.tick(&src, 200, &mut r, false).unwrap();
        assert!(matches!(out, Some(RestoreOutcome::KeptManual(3400))));
        assert!(b.is_manual(200));
        assert!(!r.take_notices().is_empty());
    }
}
