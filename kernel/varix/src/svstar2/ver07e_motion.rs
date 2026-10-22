//! VE-F3445 · 动效速度变体运行时 —— 三档时长、层级递增、减弱动效联动
//!
//! ## 职责拆解
//! 1. **三档**（`MotionGear`）：舒缓 1500‰ / 标准 1000‰ / 极速 500‰。
//! 2. **层级递增**（`duration_ms`）：层越深过渡越长。原因不是审美，是**感知**：
//!    一个弹层里的按钮，用户先看到弹层展开再看到按钮变色；两者同时结束会让
//!    「层级感」消失。每深一层 +200‰，上限 `MAX_MS`。
//! 3. **不为零硬线**（`MIN_MS = 40`）：任何档位、任何深度都不得产出 0ms。
//!    0ms 不是「很快」，是「瞬间切换」——那是另一种策略（`InstantCut`），
//!    必须显式选择，不能作为时长计算的下溢结果偷偷出现。
//! 4. **减弱动效联动**（`effective`）：系统开启「减弱动效」时，**强制**降到极速档
//!    并关闭装饰（阴影/辉光）。这是覆盖而非协商：无障碍设置优先于用户审美选择。
//!    覆盖必须**可见告知**（`notice`），否则用户会以为动效坏了。
//! 5. **可逆**（`restore`）：关闭减弱动效后回到用户原本选的档，而不是停在极速。
//!    把「临时覆盖」实现成「永久改写」是这类系统最常见的缺陷。
//!
//! ## 为什么这么拆
//! 动效参数最容易变成「一个 duration 常量乘以系数」。但真实系统里它至少有三层：
//! 档位系数、层级递增、无障碍覆盖。三者叠在一起还会互相打架（减弱动效 + 深层级
//! 到底该多长？），所以本册把「叠序」写死：**先层级递增，再档位缩放，最后覆盖**。
//! 覆盖在最后，才能真的覆盖住一切。
//!
//! ## 复杂度
//! - 单次时长求值 O(1)；覆盖求值 O(1)。
//! - 全档审计 O(档 × 深度) = 3 × 6 = 18 项。
//!
//! ## 零 panic 面
//! - 全 u32，乘法走 u64；`saturating_*` 处理深度与上限。
//! - 无索引、无 `unwrap` / `expect`。

use alloc::format;
use alloc::string::ToString;
use alloc::string::String;
use alloc::vec::Vec;

/// 时长下限（ms）。低于此值肉眼视为瞬时，不配叫过渡。
pub const MIN_MS: u32 = 40;
/// 时长上限（ms）。超过 2s 的过渡会被用户当成卡顿。
pub const MAX_MS: u32 = 2000;
/// 基准时长（ms）：标准档、层级 0、单一控件。
pub const BASE_MS: u32 = 160;
/// 每深一层的增幅（千分比）。200‰ = 每层多两成。
pub const DEPTH_STEP_PERMILLE: u32 = 200;
/// 层级上限。超过 5 层的嵌套本身就该重构，时长不再继续加。
pub const DEPTH_CAP: u8 = 5;

/// 诊断码（E18 段 · 5627~5636）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum MotionCode {
    /// 档位越界。
    LevelOutOfRange = 5627,
    /// 时长下溢到 0（瞬间切换伪装成过渡）。
    ZeroDuration = 5628,
    /// 时长超过上限。
    Overlong = 5629,
    /// 层级越深时长反而更短。
    DepthNonMonotonic = 5630,
    /// 减弱动效未生效（覆盖失败）。
    ReduceIgnored = 5631,
    /// 减弱动效下装饰未关闭。
    DecorationNotDropped = 5632,
    /// 覆盖未告知用户。
    NoticeMissing = 5633,
    /// 关闭减弱动效后未恢复原档。
    NotRestored = 5634,
    /// 三档系数不单调。
    GearNonMonotonic = 5635,
    /// 计算溢出。
    Overflow = 5636,
}

impl MotionCode {
    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn text(self) -> &'static str {
        match self {
            MotionCode::LevelOutOfRange => "档位越界",
            MotionCode::ZeroDuration => "时长为零（瞬间切换伪装成过渡）",
            MotionCode::Overlong => "时长超过上限",
            MotionCode::DepthNonMonotonic => "层更深但时长更短",
            MotionCode::ReduceIgnored => "减弱动效未生效",
            MotionCode::DecorationNotDropped => "减弱动效下装饰未关闭",
            MotionCode::NoticeMissing => "覆盖未告知用户",
            MotionCode::NotRestored => "关闭减弱动效后未恢复原档",
            MotionCode::GearNonMonotonic => "三档系数不单调",
            MotionCode::Overflow => "计算溢出",
        }
    }
}

/// 三档动效速度。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum MotionGear {
    Calm,
    Standard,
    Quick,
}

impl MotionGear {
    pub const ALL: [MotionGear; 3] = [MotionGear::Calm, MotionGear::Standard, MotionGear::Quick];

    pub fn of_level(level: u8) -> Result<Self, MotionCode> {
        match level {
            0 => Ok(MotionGear::Calm),
            1 => Ok(MotionGear::Standard),
            2 => Ok(MotionGear::Quick),
            _ => Err(MotionCode::LevelOutOfRange),
        }
    }

    pub fn level(self) -> u8 {
        match self {
            MotionGear::Calm => 0,
            MotionGear::Standard => 1,
            MotionGear::Quick => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            MotionGear::Calm => "calm",
            MotionGear::Standard => "standard",
            MotionGear::Quick => "quick",
        }
    }

    /// 时长系数（千分比）。
    pub fn permille(self) -> u32 {
        match self {
            MotionGear::Calm => 1500,
            MotionGear::Standard => 1000,
            MotionGear::Quick => 500,
        }
    }

    /// 时长（ms）。先按层级放大，再按档位缩放，最后夹到 [MIN_MS, MAX_MS]。
    pub fn duration_ms(self, depth: u8) -> u32 {
        let d = if depth > DEPTH_CAP { DEPTH_CAP } else { depth };
        // 层级因子 = 1000 + depth × 200（千分比）
        let depth_factor = 1000u32.saturating_add(u32::from(d) * DEPTH_STEP_PERMILLE);
        let raw = u64::from(BASE_MS) * u64::from(depth_factor) * u64::from(self.permille());
        // 除以 1_000_000（两个千分比连乘）
        let ms = raw / 1_000_000;
        let ms = if ms > u64::from(u32::MAX) {
            u32::MAX
        } else {
            ms as u32
        };
        ms.clamp(MIN_MS, MAX_MS)
    }
}

/// 装饰开关。减弱动效时全部关闭。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Decorations {
    pub shadow: bool,
    pub glow: bool,
    pub parallax: bool,
}

impl Decorations {
    pub const ALL_ON: Decorations = Decorations {
        shadow: true,
        glow: true,
        parallax: true,
    };

    pub const NONE: Decorations = Decorations {
        shadow: false,
        glow: false,
        parallax: false,
    };

    pub fn any(&self) -> bool {
        self.shadow || self.glow || self.parallax
    }
}

/// 生效的动效配置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Effective {
    pub gear: MotionGear,
    pub decorations: Decorations,
    /// 是否被无障碍设置覆盖。
    pub overridden: bool,
}

impl Effective {
    /// 生效时长。
    pub fn duration_ms(&self, depth: u8) -> u32 {
        self.gear.duration_ms(depth)
    }

    /// 覆盖告知文案。为空即「没有覆盖」，测试与 UI 都靠这个判断。
    pub fn notice(&self) -> Option<String> {
        if self.overridden {
            Some("已按「减弱动效」设置使用最快过渡并关闭装饰效果".into())
        } else {
            None
        }
    }
}

/// 求生效配置。`reduce_motion` 为真时**强制**极速档 + 去装饰。
///
/// 覆盖是单向的：只能在「更快、更少装饰」的方向上作用。反向覆盖
/// （用户关了减弱动效却被变体压成舒缓档）不存在——减弱动效的关闭
/// 只意味着「不再强制」，并不意味着「必须变慢」。
pub fn effective(gear: MotionGear, reduce_motion: bool) -> Effective {
    if reduce_motion {
        Effective {
            gear: MotionGear::Quick,
            decorations: Decorations::NONE,
            overridden: true,
        }
    } else {
        Effective {
            gear,
            decorations: Decorations::ALL_ON,
            overridden: false,
        }
    }
}

/// 恢复：从覆盖态回到用户原档。`remembered` 是被覆盖前的用户选择。
/// 这样「关掉减弱动效」得到的是用户原来的档，而不是极速档。
pub fn restore(remembered: MotionGear) -> Effective {
    Effective {
        gear: remembered,
        decorations: Decorations::ALL_ON,
        overridden: false,
    }
}

/// 单档审计。
pub fn audit_one(gear: MotionGear) -> Vec<MotionCode> {
    let mut out = Vec::new();
    let mut prev = 0u32;
    for depth in 0..=DEPTH_CAP {
        let ms = gear.duration_ms(depth);
        if ms < MIN_MS {
            out.push(MotionCode::ZeroDuration);
        }
        if ms > MAX_MS {
            out.push(MotionCode::Overlong);
        }
        if depth > 0 && ms < prev {
            out.push(MotionCode::DepthNonMonotonic);
        }
        prev = ms;
    }
    out
}

/// 全档审计：单档 + 单调性 + 覆盖链路。
pub fn audit() -> Vec<MotionCode> {
    let mut out = Vec::new();
    for g in MotionGear::ALL {
        for c in audit_one(g) {
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    // 三档系数必须递减（calm 最慢、quick 最快）
    let a = MotionGear::Calm.permille();
    let b = MotionGear::Standard.permille();
    let c = MotionGear::Quick.permille();
    if !(a > b && b > c) {
        out.push(MotionCode::GearNonMonotonic);
    }
    // 覆盖链路：减弱动效必须真的更快且去装饰且带告知
    let reduced = effective(MotionGear::Calm, true);
    if reduced.gear != MotionGear::Quick {
        out.push(MotionCode::ReduceIgnored);
    }
    if reduced.decorations.any() {
        out.push(MotionCode::DecorationNotDropped);
    }
    if reduced.notice().is_none() {
        out.push(MotionCode::NoticeMissing);
    }
    if reduced.duration_ms(DEPTH_CAP) >= MotionGear::Calm.duration_ms(0) {
        out.push(MotionCode::ReduceIgnored);
    }
    // 恢复链路
    let restored = restore(MotionGear::Calm);
    if restored.gear != MotionGear::Calm || restored.overridden {
        out.push(MotionCode::NotRestored);
    }
    out
}

/// 读屏清单：三档 × 四个深度的时长表。
pub fn duration_table() -> Vec<String> {
    let mut out = Vec::new();
    out.push("动效档位 | d0 | d1 | d2 | d3 | d4 | d5（ms）".to_string());
    for g in MotionGear::ALL {
        let cells: Vec<String> = (0..=DEPTH_CAP)
            .map(|d| format!("{}", g.duration_ms(d)))
            .collect();
        out.push(format!("{:<8} {}", g.name(), cells.join(" | ")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_gears_and_levels() {
        assert_eq!(MotionGear::of_level(2), Ok(MotionGear::Quick));
        assert_eq!(MotionGear::of_level(3), Err(MotionCode::LevelOutOfRange));
        assert!(MotionGear::Calm.permille() > MotionGear::Standard.permille());
        assert!(MotionGear::Standard.permille() > MotionGear::Quick.permille());
    }

    #[test]
    fn durations_grow_with_depth_and_never_zero() {
        for g in MotionGear::ALL {
            let mut prev = 0u32;
            for d in 0..=DEPTH_CAP {
                let ms = g.duration_ms(d);
                assert!(ms >= MIN_MS, "{} d{} = {}ms 低于下限", g.name(), d, ms);
                assert!(ms <= MAX_MS);
                if d > 0 {
                    assert!(ms >= prev, "{} 深度 {} 反而更短", g.name(), d);
                }
                prev = ms;
            }
        }
        // 标准档 d0 = 160 * 1000 * 1000 / 1e6 = 160
        assert_eq!(MotionGear::Standard.duration_ms(0), 160);
        // 极速档 d0 = 160 * 500 = 80
        assert_eq!(MotionGear::Quick.duration_ms(0), 80);
        // 舒缓档 d0 = 240
        assert_eq!(MotionGear::Calm.duration_ms(0), 240);
    }

    #[test]
    fn depth_beyond_cap_stops_growing() {
        for g in MotionGear::ALL {
            assert_eq!(g.duration_ms(DEPTH_CAP), g.duration_ms(200));
        }
    }

    #[test]
    fn extreme_depth_cannot_overflow() {
        for g in MotionGear::ALL {
            let ms = g.duration_ms(u8::MAX);
            assert!(ms <= MAX_MS);
            assert!(ms >= MIN_MS);
        }
    }

    #[test]
    fn reduce_motion_forces_quick_and_drops_decoration() {
        let e = effective(MotionGear::Calm, true);
        assert_eq!(e.gear, MotionGear::Quick);
        assert!(!e.decorations.any());
        assert!(e.overridden);
        assert!(e.notice().is_some(), "覆盖必须告知");
        // 覆盖后的时长必须严格快于原来的慢档
        assert!(e.duration_ms(0) < MotionGear::Calm.duration_ms(0));
    }

    #[test]
    fn normal_path_keeps_user_choice() {
        let e = effective(MotionGear::Calm, false);
        assert_eq!(e.gear, MotionGear::Calm);
        assert!(e.decorations.any());
        assert!(!e.overridden);
        assert!(e.notice().is_none());
        assert_eq!(e.duration_ms(2), MotionGear::Calm.duration_ms(2));
    }

    #[test]
    fn restore_returns_to_user_choice_not_to_quick() {
        let remembered = MotionGear::Calm;
        let after = restore(remembered);
        assert_eq!(after.gear, MotionGear::Calm);
        assert!(!after.overridden);
        assert!(after.decorations.any());
        // 覆盖期间用户可能改过档；restore 用的是「记住的那个」
        assert_ne!(after.gear, MotionGear::Quick);
    }

    #[test]
    fn full_audit_is_clean() {
        assert!(audit().is_empty(), "{:?}", audit());
    }

    #[test]
    fn duration_table_is_readable() {
        let t = duration_table();
        assert_eq!(t.len(), 4);
        assert!(t[0].contains("动效档位"));
        assert!(t[1].contains("calm"));
    }
}
