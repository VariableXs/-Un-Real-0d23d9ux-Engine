//! VE-F3449 · 变体与无障碍联动 —— 强制覆盖、优先级、互斥级联、可见告知
//!
//! ## 职责拆解
//! 1. **覆盖**（`A11y`）：三项无障碍设置各锁一条轴——
//!    大字号 → 字阶松（`TypeScale=2`）、减弱动效 → 动效极速（`Motion=2`）、
//!    高对比 → 焦点环醒目（`FocusRing=2`）。
//! 2. **优先级**（`apply`）：**用户无障碍设置 > 变体预设**，单向不可协商。
//!    变体是审美选择，无障碍是可用性需求；两者冲突时后者赢，没有例外。
//! 3. **互斥级联**（`OverrideReason::MutexCascade`）：覆盖会**制造**新的互斥——
//!    例如用户选了紧凑密度，再开大字号，就撞上「紧凑 + 松散字阶」。
//!    此时不能拒绝大字号（那是把无障碍需求挡回去），只能把密度抬到标准，
//!    并**再记一条覆盖**。级联是这套系统的必要复杂度，藏起来才是缺陷。
//! 4. **可见告知**（`notice_lines`）：每条覆盖都要有一句人话。
//!    「设置没生效也没说是为什么」是这类系统最招人恨的形态。
//! 5. **可审计**（`audit`）：三条预置 × 八种开关组合，全部必须收敛到合法组合。
//!    收敛不了就说明级联逻辑有环——那必须在上线前发现。
//!
//! ## 为什么这么拆
//! 无障碍联动最容易写成「if 大字号 then 字号 = 大」，然后在大字号 + 紧凑密度时
//! 撞进互斥表，最后把大字号静默降级。本册把顺序倒过来：**先执行覆盖，
//! 再修互斥**，且修互斥的动作本身也记成覆盖并告知。这样最终状态永远合法，
//! 用户看到的每一条变化都有解释。
//!
//! ## 复杂度
//! - 单次 apply：O(轴 × 迭代次数)，迭代上限 `MAX_CASCADE`(3) ⇒ 常数。
//! - 全量审计：O(预置 3 × 开关 8 × 常数) = 24 次 apply。
//!
//! ## 零 panic 面
//! - 无索引；`Vec` 只 push；迭代次数硬上限防死循环。
//! - 无 `unwrap` / `expect`。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::ver07a_variants::{conflicts, presets, Axis, Combo, LEVELS};

/// 级联修正的最大迭代次数。三条互斥项最多触发三次连锁，再多就是环。
pub const MAX_CASCADE: usize = 3;

/// 诊断码（E18 段 · 5667~5676）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum A11yCode {
    /// 覆盖未生效（被变体压回去了）。
    OverrideLost = 5667,
    /// 级联未能收敛（互斥修不掉）。
    CascadeUnsettled = 5668,
    /// 覆盖未告知。
    NoticeMissing = 5669,
    /// 覆盖清单与实际组合不一致。
    OverrideSkew = 5670,
    /// 档位越界。
    LevelOutOfRange = 5671,
    /// 用户关掉了设置但覆盖仍在。
    StaleOverride = 5672,
    /// 覆盖把组合推到了更差的可读性。
    ReadabilityRegression = 5673,
    /// 审计样本为空。
    EmptySample = 5674,
    /// 设置项互相矛盾（未来扩展占位：当前三项互不矛盾）。
    ConflictingSettings = 5675,
    /// 计算溢出。
    Overflow = 5676,
}

impl A11yCode {
    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn text(self) -> &'static str {
        match self {
            A11yCode::OverrideLost => "覆盖未生效",
            A11yCode::CascadeUnsettled => "级联未收敛",
            A11yCode::NoticeMissing => "覆盖未告知",
            A11yCode::OverrideSkew => "覆盖清单与实际不一致",
            A11yCode::LevelOutOfRange => "档位越界",
            A11yCode::StaleOverride => "设置已关但覆盖仍在",
            A11yCode::ReadabilityRegression => "覆盖导致可读性倒退",
            A11yCode::EmptySample => "审计样本为空",
            A11yCode::ConflictingSettings => "设置项互相矛盾",
            A11yCode::Overflow => "计算溢出",
        }
    }
}

/// 无障碍设置。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct A11y {
    pub large_text: bool,
    pub reduce_motion: bool,
    pub high_contrast: bool,
}

impl A11y {
    pub const NONE: A11y = A11y {
        large_text: false,
        reduce_motion: false,
        high_contrast: false,
    };

    pub fn any(&self) -> bool {
        self.large_text || self.reduce_motion || self.high_contrast
    }

    /// 八种开关组合，用于全量审计。
    pub fn all_combinations() -> Vec<A11y> {
        let mut out = Vec::with_capacity(8);
        for bits in 0u8..8 {
            out.push(A11y {
                large_text: bits & 0b001 != 0,
                reduce_motion: bits & 0b010 != 0,
                high_contrast: bits & 0b100 != 0,
            });
        }
        out
    }
}

/// 覆盖原因。`MutexCascade` 是「为了让别的覆盖合法而做的覆盖」，必须能区分——
/// 否则用户会看到一个他没要求过的密度变化，却找不到解释。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverrideReason {
    LargeText,
    ReduceMotion,
    HighContrast,
    MutexCascade,
}

impl OverrideReason {
    pub fn label(self) -> &'static str {
        match self {
            OverrideReason::LargeText => "大字号",
            OverrideReason::ReduceMotion => "减弱动效",
            OverrideReason::HighContrast => "高对比",
            OverrideReason::MutexCascade => "互斥级联",
        }
    }

    /// 是否为「用户直接要求」的覆盖（级联不是）。
    pub fn is_direct(self) -> bool {
        !matches!(self, OverrideReason::MutexCascade)
    }
}

/// 一条覆盖记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Override {
    pub axis: Axis,
    pub from: u8,
    pub to: u8,
    pub reason: OverrideReason,
}

impl Override {
    pub fn notice(&self) -> String {
        match self.reason {
            OverrideReason::LargeText => format!(
                "「大字号」已将{}调至 {}",
                self.axis.label(),
                self.axis.level_name(self.to)
            ),
            OverrideReason::ReduceMotion => format!(
                "「减弱动效」已将{}调至 {}",
                self.axis.label(),
                self.axis.level_name(self.to)
            ),
            OverrideReason::HighContrast => format!(
                "「高对比」已将{}调至 {}",
                self.axis.label(),
                self.axis.level_name(self.to)
            ),
            OverrideReason::MutexCascade => format!(
                "为满足无障碍设置，{}已从 {} 调整为 {}",
                self.axis.label(),
                self.axis.level_name(self.from),
                self.axis.level_name(self.to)
            ),
        }
    }
}

/// 联动结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Resolution {
    pub combo: Combo,
    pub overrides: Vec<Override>,
}

impl Resolution {
    /// 强制覆盖条数（不含级联）。
    pub fn forced(&self) -> usize {
        self.overrides
            .iter()
            .filter(|o| o.reason.is_direct())
            .count()
    }

    /// 级联条数。
    pub fn cascaded(&self) -> usize {
        self.overrides
            .iter()
            .filter(|o| !o.reason.is_direct())
            .count()
    }

    /// 读屏/告知清单。每条覆盖一句，末尾一句汇总。
    pub fn notice_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for o in &self.overrides {
            out.push(o.notice());
        }
        if self.cascaded() > 0 {
            out.push(format!(
                "另有 {} 项自动调整用于避免布局冲突，可在设置中查看详情",
                self.cascaded()
            ));
        }
        out
    }
}

/// 单个设置 → 它锁定的（轴, 档）。
fn locked_level(setting_on: bool, axis: Axis, level: u8) -> Option<(Axis, u8)> {
    if setting_on {
        Some((axis, level))
    } else {
        None
    }
}

/// 应用无障碍覆盖。
///
/// 顺序写死：**先直接覆盖，再修互斥**。反过来会先算出「无需修正」的结论，
/// 再把冲突漏给下游——那正是这个功能最典型的缺陷形态。
pub fn apply(base: Combo, a11y: &A11y) -> Result<Resolution, A11yCode> {
    let mut overrides: Vec<Override> = Vec::new();
    let mut combo = base;

    // 阶段一：直接覆盖。
    let locks = [
        locked_level(a11y.large_text, Axis::TypeScale, 2).map(|v| (v, OverrideReason::LargeText)),
        locked_level(a11y.reduce_motion, Axis::Motion, 2).map(|v| (v, OverrideReason::ReduceMotion)),
        locked_level(a11y.high_contrast, Axis::FocusRing, 2)
            .map(|v| (v, OverrideReason::HighContrast)),
    ];
    for entry in locks.iter().flatten() {
        let ((axis, level), reason) = *entry;
        if level >= LEVELS {
            return Err(A11yCode::LevelOutOfRange);
        }
        let from = combo.level(axis);
        if from != level {
            combo = combo.with(axis, level).map_err(|_| A11yCode::LevelOutOfRange)?;
            overrides.push(Override {
                axis,
                from,
                to: level,
                reason,
            });
        }
    }

    // 阶段二：修互斥。只往「更宽松」的方向修，绝不为了消冲突把字阶压回去
    // ——那等于用可读性去换布局整齐。
    let mut guard = 0usize;
    while !conflicts(&combo).is_empty() {
        guard += 1;
        if guard > MAX_CASCADE {
            return Err(A11yCode::CascadeUnsettled);
        }
        let mut settled = false;
        // 冲突项里若含密度，抬密度到标准档；否则抬半径（唯一另一条轴）。
        for m in conflicts(&combo) {
            let target_axis = if m.a.axis == Axis::Density || m.b.axis == Axis::Density {
                Axis::Density
            } else {
                Axis::Radius
            };
            let from = combo.level(target_axis);
            let to = if from < 1 { 1 } else { from };
            if to != from {
                combo = combo
                    .with(target_axis, to)
                    .map_err(|_| A11yCode::LevelOutOfRange)?;
                overrides.push(Override {
                    axis: target_axis,
                    from,
                    to,
                    reason: OverrideReason::MutexCascade,
                });
                settled = true;
                break;
            }
        }
        if !settled {
            // 已经在中档及以上仍冲突：说明互斥表本身要求不可满足，报未收敛。
            return Err(A11yCode::CascadeUnsettled);
        }
    }

    // 阶段三：可读性不得倒退——覆盖后的字阶不得低于原组合。
    if combo.level(Axis::TypeScale) < base.level(Axis::TypeScale) {
        return Err(A11yCode::ReadabilityRegression);
    }

    Ok(Resolution { combo, overrides })
}

/// 自检：三条预置 × 八种开关，全部必须收敛、必须每条覆盖都有告知。
pub fn audit() -> Vec<A11yCode> {
    let mut out = Vec::new();
    let table = presets();
    if table.is_empty() {
        out.push(A11yCode::EmptySample);
        return out;
    }
    let combos = A11y::all_combinations();
    if combos.is_empty() {
        out.push(A11yCode::EmptySample);
    }
    for p in &table {
        for a in &combos {
            match apply(p.combo, a) {
                Ok(r) => {
                    if !conflicts(&r.combo).is_empty() {
                        out.push(A11yCode::CascadeUnsettled);
                    }
                    // 直接覆盖必须真的落地
                    if a.large_text && r.combo.level(Axis::TypeScale) != 2 {
                        out.push(A11yCode::OverrideLost);
                    }
                    if a.reduce_motion && r.combo.level(Axis::Motion) != 2 {
                        out.push(A11yCode::OverrideLost);
                    }
                    if a.high_contrast && r.combo.level(Axis::FocusRing) != 2 {
                        out.push(A11yCode::OverrideLost);
                    }
                    // 每条覆盖都必须有告知
                    if !r.overrides.is_empty() && r.notice_lines().is_empty() {
                        out.push(A11yCode::NoticeMissing);
                    }
                    // 未开任何设置时不得产生覆盖
                    if !a.any() && !r.overrides.is_empty() {
                        out.push(A11yCode::StaleOverride);
                    }
                }
                Err(e) => out.push(e),
            }
        }
    }
    out
}

/// 读屏清单：三项设置各自锁定哪条轴。
pub fn setting_lines() -> Vec<String> {
    let mut out = Vec::new();
    out.push("设置 | 锁定轴 | 目标档".to_string());
    out.push(format!("大字号 | {} | loose", Axis::TypeScale.label()));
    out.push(format!("减弱动效 | {} | quick", Axis::Motion.label()));
    out.push(format!("高对比 | {} | bold", Axis::FocusRing.label()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Combo {
        Combo::default_combo()
    }

    #[test]
    fn no_settings_means_no_overrides() {
        let r = apply(base(), &A11y::NONE).unwrap_or_else(|_| panic!("必须成功"));
        assert_eq!(r.combo, base());
        assert!(r.overrides.is_empty());
        assert!(r.notice_lines().is_empty());
    }

    #[test]
    fn large_text_beats_preset_and_is_announced() {
        let work = match presets().get(1) {
            Some(p) => p.combo,
            None => panic!("预置不足"),
        };
        // work 的字阶是 tight(0)，大字号必须抬到 loose(2)
        assert_eq!(work.level(Axis::TypeScale), 0);
        let a = A11y {
            large_text: true,
            ..A11y::NONE
        };
        let r = apply(work, &a).unwrap_or_else(|_| panic!("必须成功"));
        assert_eq!(r.combo.level(Axis::TypeScale), 2);
        assert_eq!(r.forced(), 1);
        let notice = r.notice_lines().join(" / ");
        assert!(notice.contains("大字号"), "{}", notice);
    }

    #[test]
    fn mutex_conflict_is_cascaded_not_refused() {
        // 紧凑密度 + 大字号 ⇒ 撞「紧凑 + 松散字阶」，必须抬密度而不是压字号
        let compact = base().with(Axis::Density, 0).unwrap_or(base());
        let a = A11y {
            large_text: true,
            ..A11y::NONE
        };
        let r = apply(compact, &a).unwrap_or_else(|_| panic!("必须成功"));
        assert!(conflicts(&r.combo).is_empty(), "结果必须合法");
        assert_eq!(r.combo.level(Axis::TypeScale), 2, "字阶不能为了布局让步");
        assert!(r.combo.level(Axis::Density) >= 1, "密度必须被抬起");
        assert_eq!(r.cascaded(), 1);
        let notice = r.notice_lines().join(" / ");
        assert!(notice.contains("互斥") || notice.contains("自动调整"), "{}", notice);
    }

    #[test]
    fn all_three_settings_together_converge() {
        let a = A11y {
            large_text: true,
            reduce_motion: true,
            high_contrast: true,
        };
        let r = apply(base(), &a).unwrap_or_else(|_| panic!("必须成功"));
        assert_eq!(r.combo.level(Axis::TypeScale), 2);
        assert_eq!(r.combo.level(Axis::Motion), 2);
        assert_eq!(r.combo.level(Axis::FocusRing), 2);
        assert_eq!(r.forced(), 3);
        assert!(conflicts(&r.combo).is_empty());
    }

    #[test]
    fn turning_off_settings_leaves_no_stale_override() {
        let a = A11y {
            large_text: true,
            ..A11y::NONE
        };
        let on = apply(base(), &a).unwrap_or_else(|_| panic!("必须成功"));
        assert!(!on.overrides.is_empty());
        let off = apply(base(), &A11y::NONE).unwrap_or_else(|_| panic!("必须成功"));
        assert!(off.overrides.is_empty());
        assert_eq!(off.combo, base());
    }

    #[test]
    fn readability_never_regresses() {
        for p in presets() {
            for a in A11y::all_combinations() {
                let r = apply(p.combo, &a).unwrap_or_else(|_| panic!("必须成功"));
                assert!(r.combo.level(Axis::TypeScale) >= p.combo.level(Axis::TypeScale));
            }
        }
    }

    #[test]
    fn full_audit_is_clean() {
        let issues = audit();
        assert!(issues.is_empty(), "{:?}", issues);
    }

    #[test]
    fn settings_have_human_notices() {
        let s = setting_lines();
        assert_eq!(s.len(), 4);
        assert!(s[0].contains("设置"));
        // 每条 notice 必须提到设置名，而不是只报档位
        let o = Override {
            axis: Axis::Motion,
            from: 1,
            to: 2,
            reason: OverrideReason::ReduceMotion,
        };
        assert!(o.notice().contains("减弱动效"));
        let c = Override {
            axis: Axis::Density,
            from: 0,
            to: 1,
            reason: OverrideReason::MutexCascade,
        };
        assert!(c.notice().contains("compact"));
        assert!(c.notice().contains("standard"));
    }

    #[test]
    fn override_counters_split_direct_and_cascade() {
        let compact = base().with(Axis::Density, 0).unwrap_or(base());
        let a = A11y {
            large_text: true,
            high_contrast: true,
            ..A11y::NONE
        };
        let r = apply(compact, &a).unwrap_or_else(|_| panic!("必须成功"));
        assert_eq!(r.forced(), 2, "大字号 + 高对比");
        assert_eq!(r.cascaded(), 1, "密度级联");
        assert_eq!(r.overrides.len(), 3);
    }
}
