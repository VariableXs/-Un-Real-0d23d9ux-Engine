//! ve179_knobpanel ↔ VE-F0179 实时调参面板
//!
//! 渲染参数旋钮实时调节：旋钮注册（名称×范围×步进×即时生效性）、调节即时应用（不重启）、
//! 当前值与默认值并列显示、一键恢复默认。
//!
//! 判据：旋钮注册、即时应用、失败回滚、一键默认。

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::float::FloatExt;

/// 旋钮生效性。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effectiveness {
    /// 即时生效（应用失败可回滚）。
    Immediate,
    /// 需重启生效（标注不假装即时）。
    RequiresRestart,
}

/// 旋钮注册条目。
#[derive(Debug, Clone)]
pub struct Knob {
    /// 旋钮名称（唯一键）。
    pub name: String,
    /// 最小值（含）。
    pub min: f64,
    /// 最大值（含）。
    pub max: f64,
    /// 步进粒度。
    pub step: f64,
    /// 当前值。
    pub current: f64,
    /// 默认值（注册时锁定）。
    pub default: f64,
    /// 生效性。
    pub effect: Effectiveness,
    /// 描述文本。
    pub desc: String,
}

/// 调参错误码。
pub mod err {
    /// 旋钮不存在。
    pub const E_NO_KNOB: u16 = 0x7901;
    /// 范围无效（min >= max 或 step <= 0）。
    pub const E_BAD_RANGE: u16 = 0x7902;
    /// 默认值超出范围。
    pub const E_DEFAULT_OOB: u16 = 0x7903;
    /// 应用失败（回滚旧值）。
    pub const E_APPLY_FAIL: u16 = 0x7904;
    /// 旋钮名称重复。
    pub const E_DUPLICATE: u16 = 0x7905;
    /// 步进不对齐。
    pub const E_STEP_MISALIGN: u16 = 0x7906;
}

/// 应用回调结果。
pub type ApplyResult = Result<(), u16>;

/// 实时调参面板。
pub struct KnobPanel {
    /// 旋钮表。
    knobs: BTreeMap<String, Knob>,
    /// 被钳制次数（统计）。
    clamped_count: u64,
    /// 回滚次数（统计）。
    rollback_count: u64,
    /// 应用成功次数。
    apply_count: u64,
}

impl KnobPanel {
    /// 新建空面板。
    pub fn new() -> Self {
        Self {
            knobs: BTreeMap::new(),
            clamped_count: 0,
            rollback_count: 0,
            apply_count: 0,
        }
    }

    /// 注册旋钮。
    pub fn register(
        &mut self,
        name: &str,
        min: f64,
        max: f64,
        step: f64,
        default: f64,
        effect: Effectiveness,
        desc: &str,
    ) -> Result<(), u16> {
        if name.is_empty() {
            return Err(err::E_NO_KNOB);
        }
        if self.knobs.contains_key(name) {
            return Err(err::E_DUPLICATE);
        }
        if min >= max || step <= 0.0 {
            return Err(err::E_BAD_RANGE);
        }
        if default < min || default > max {
            return Err(err::E_DEFAULT_OOB);
        }
        let knob = Knob {
            name: name.to_string(),
            min,
            max,
            step,
            current: default,
            default,
            effect,
            desc: desc.to_string(),
        };
        self.knobs.insert(name.to_string(), knob);
        Ok(())
    }

    /// 获取当前值。
    pub fn get(&self, name: &str) -> Option<f64> {
        self.knobs.get(name).map(|k| k.current)
    }

    /// 获取默认值。
    pub fn get_default(&self, name: &str) -> Option<f64> {
        self.knobs.get(name).map(|k| k.default)
    }

    /// 获取旋钮完整信息。
    pub fn knob(&self, name: &str) -> Option<&Knob> {
        self.knobs.get(name)
    }

    /// 调节旋钮到指定值（钳制到合法域）。
    ///
    /// 对即时生效旋钮，调用 `apply_fn` 应用；失败则回滚旧值。
    /// 对需重启旋钮，直接存储不调用 apply_fn，返回 Ok 标注需重启。
    pub fn set<F>(
        &mut self,
        name: &str,
        value: f64,
        apply_fn: &F,
    ) -> Result<(f64, bool), u16>
    where
        F: Fn(&str, f64) -> ApplyResult,
    {
        let knob = self.knobs.get_mut(name).ok_or(err::E_NO_KNOB)?;

        // 钳制到合法域
        let clamped = value.max(knob.min).min(knob.max);
        if clamped != value {
            self.clamped_count += 1;
        }

        // 步进对齐（舍入到最近步进点）
        let aligned = if knob.step > 0.0 {
            let base = knob.min;
            let steps = ((clamped - base) / knob.step).m_round();
            (base + steps * knob.step).min(knob.max).max(knob.min)
        } else {
            clamped
        };

        let old_value = knob.current;

        // 需重启旋钮：直接存储，不假装即时
        if knob.effect == Effectiveness::RequiresRestart {
            knob.current = aligned;
            return Ok((aligned, false)); // false = 非即时生效
        }

        // 即时生效：先试应用
        match apply_fn(name, aligned) {
            Ok(()) => {
                knob.current = aligned;
                self.apply_count += 1;
                Ok((aligned, true))
            }
            Err(_) => {
                // 回滚旧值
                self.rollback_count += 1;
                knob.current = old_value;
                Err(err::E_APPLY_FAIL)
            }
        }
    }

    /// 一键恢复默认值。
    pub fn reset_to_default<F>(&mut self, name: &str, apply_fn: &F) -> Result<f64, u16>
    where
        F: Fn(&str, f64) -> ApplyResult,
    {
        let default = self.knobs.get(name).map(|k| k.default).ok_or(err::E_NO_KNOB)?;
        self.set(name, default, apply_fn).map(|(v, _)| v)
    }

    /// 全部旋钮一键恢复默认。
    pub fn reset_all<F>(&mut self, apply_fn: &F) -> (usize, usize)
    where
        F: Fn(&str, f64) -> ApplyResult,
    {
        let names: Vec<String> = self.knobs.keys().cloned().collect();
        let mut ok = 0;
        let mut fail = 0;
        for name in &names {
            match self.reset_to_default(name.as_str(), apply_fn) {
                Ok(_) => ok += 1,
                Err(_) => fail += 1,
            }
        }
        (ok, fail)
    }

    /// 列出所有旋钮（含当前值与默认值）。
    pub fn list(&self) -> Vec<&Knob> {
        self.knobs.values().collect()
    }

    /// 当前值与默认值并列显示数据。
    pub fn display_pair(&self, name: &str) -> Option<(f64, f64, bool)> {
        self.knobs
            .get(name)
            .map(|k| (k.current, k.default, k.effect == Effectiveness::Immediate))
    }

    /// 统计信息。
    pub fn stats(&self) -> (u64, u64, u64) {
        (self.clamped_count, self.rollback_count, self.apply_count)
    }
}

impl Default for KnobPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_apply(_name: &str, _v: f64) -> ApplyResult {
        Ok(())
    }
    fn fail_apply(_name: &str, _v: f64) -> ApplyResult {
        Err(0xAA)
    }

    #[test]
    fn test_register_and_get() {
        let mut p = KnobPanel::new();
        p.register("brightness", 0.0, 100.0, 1.0, 50.0, Effectiveness::Immediate, "亮度")
            .unwrap();
        assert_eq!(p.get("brightness"), Some(50.0));
        assert_eq!(p.get_default("brightness"), Some(50.0));
    }

    #[test]
    fn test_clamp_oob() {
        let mut p = KnobPanel::new();
        p.register("b", 0.0, 10.0, 1.0, 5.0, Effectiveness::Immediate, "d")
            .unwrap();
        let (v, immediate) = p.set("b", 15.0, &ok_apply).unwrap();
        assert_eq!(v, 10.0);
        assert!(immediate);
        assert_eq!(p.stats().0, 1); // clamped
    }

    #[test]
    fn test_rollback_on_apply_fail() {
        let mut p = KnobPanel::new();
        p.register("b", 0.0, 10.0, 1.0, 5.0, Effectiveness::Immediate, "d")
            .unwrap();
        assert_eq!(p.set("b", 7.0, &fail_apply), Err(err::E_APPLY_FAIL));
        assert_eq!(p.get("b"), Some(5.0)); // 回滚到旧值
        assert_eq!(p.stats().1, 1); // rollback
    }

    #[test]
    fn test_requires_restart_not_immediate() {
        let mut p = KnobPanel::new();
        p.register("r", 0.0, 10.0, 1.0, 5.0, Effectiveness::RequiresRestart, "d")
            .unwrap();
        let (v, immediate) = p.set("r", 7.0, &ok_apply).unwrap();
        assert_eq!(v, 7.0);
        assert!(!immediate); // 标注非即时
    }

    #[test]
    fn test_reset_to_default() {
        let mut p = KnobPanel::new();
        p.register("b", 0.0, 10.0, 1.0, 5.0, Effectiveness::Immediate, "d")
            .unwrap();
        p.set("b", 8.0, &ok_apply).unwrap();
        assert_eq!(p.reset_to_default("b", &ok_apply).unwrap(), 5.0);
        assert_eq!(p.get("b"), Some(5.0));
    }

    #[test]
    fn test_step_alignment() {
        let mut p = KnobPanel::new();
        p.register("s", 0.0, 10.0, 2.0, 0.0, Effectiveness::Immediate, "d")
            .unwrap();
        // 3.0 应对齐到 4.0（最近 2 的倍数）
        let (v, _) = p.set("s", 3.0, &ok_apply).unwrap();
        assert_eq!(v, 4.0);
    }

    #[test]
    fn test_bad_range_rejected() {
        let mut p = KnobPanel::new();
        assert_eq!(
            p.register("x", 10.0, 0.0, 1.0, 5.0, Effectiveness::Immediate, "d"),
            Err(err::E_BAD_RANGE)
        );
    }

    #[test]
    fn test_default_oob_rejected() {
        let mut p = KnobPanel::new();
        assert_eq!(
            p.register("x", 0.0, 10.0, 1.0, 15.0, Effectiveness::Immediate, "d"),
            Err(err::E_DEFAULT_OOB)
        );
    }
}
