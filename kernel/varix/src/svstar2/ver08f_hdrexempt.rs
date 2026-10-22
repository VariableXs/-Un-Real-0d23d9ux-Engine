//! VE-F3506 · HDR 内容的夜览豁免 —— 全屏 HDR 豁免 + 范围声明 + 用户可关。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3506`
//!
//! # 职责（锚点原文拆解）
//!
//! - **HDR 豁免**（[`HdrExemptor`]）：全屏 HDR 内容播放时**不套色温**——
//!   电影夜场的内容保真优先于护眼。HDR 内容的色域与亮度是内容
//!   创作者调过的，叠一层色温滤镜等于替观众重调色，越俎代庖。
//! - **范围声明**（[`ExemptScope`]）：豁免**只有全屏 HDR** 一个口子。
//!   窗口化 HDR（画中画、窗口播放）不豁免——屏幕上还有其他内容
//!   在被色温管着，只给一半屏幕开豁免会造成色温撕裂。范围是
//!   数据不是注释（[`ExemptScope::declares`]），可被 CI 检查。
//! - **豁免可关**（[`ExemptPolicy`]）：用户可以关掉豁免（「我就想
//!   夜里看电影也暖一点」是合法偏好）。豁免关掉后 HDR 全屏也套色温。
//! - **冲突仲裁**：豁免与夜览手动覆盖（F3502）同时生效时，
//!   **手动覆盖赢**——用户刚手动调的值比内容保真更近。仲裁规则
//!   公开（[`ExemptView::rule`]）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 豁免越界（窗口化 HDR 也想豁免） | 钳制为不豁免 |
//! | 声明缺失 | 补声明（默认最小范围） |
//! | 开关失效 | 修复（状态重同步） |
//!
//! # 复杂度
//!
//! 全部 O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5730 起）
// ---------------------------------------------------------------------------

/// HDR 豁免域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExemptCode {
    /// 豁免请求超出声明范围（窗口化 HDR 想豁免）。
    ScopeExceeded = 5730,
    /// 豁免声明缺失（默认补最小范围）。
    ScopeMissing = 5731,
    /// 开关状态与期望不符（重同步）。
    SwitchDesync = 5732,
    /// 豁免与手动覆盖的仲裁未决（不该出现的内部态）。
    ArbiterUndecided = 5733,
}

impl ExemptCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            ExemptCode::ScopeExceeded => "豁免请求超出声明范围",
            ExemptCode::ScopeMissing => "豁免声明缺失",
            ExemptCode::SwitchDesync => "豁免开关状态失同步",
            ExemptCode::ArbiterUndecided => "豁免仲裁未决",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, ExemptCode::ArbiterUndecided)
    }
}

// ---------------------------------------------------------------------------
// 二、范围声明
// ---------------------------------------------------------------------------

/// 豁免范围（数据化声明，可 CI 检查）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExemptScope {
    /// 最小范围：仅全屏 HDR。
    FullscreenHdrOnly,
    /// 无豁免（用户整体关闭 HDR 豁免特性时退化到此）。
    None,
}

impl ExemptScope {
    /// 声明文本（逐字公开，读屏可达）。
    pub fn declares(self) -> &'static str {
        match self {
            ExemptScope::FullscreenHdrOnly => "仅全屏 HDR 内容豁免色温",
            ExemptScope::None => "无豁免：所有内容统一套色温",
        }
    }

    /// 判定一次 HDR 呈现是否在范围内。
    pub fn covers(self, fullscreen: bool) -> bool {
        match self {
            ExemptScope::FullscreenHdrOnly => fullscreen,
            ExemptScope::None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、豁免器
// ---------------------------------------------------------------------------

/// HDR 呈现描述（来自合成器/播放器的一次询问）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HdrPresentation {
    /// 是否全屏呈现。
    pub fullscreen: bool,
    /// 是否 HDR 内容。
    pub hdr: bool,
    /// 是否处于播放中（暂停的 HDR 帧不豁免——暂停时应能看清 UI）。
    pub playing: bool,
}

/// 豁免策略（用户可关 + 范围）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExemptPolicy {
    /// 豁免特性开关（用户可关）。
    pub enabled: bool,
    /// 范围声明。
    pub scope: ExemptScope,
}

impl Default for ExemptPolicy {
    fn default() -> Self {
        ExemptPolicy {
            enabled: true,
            scope: ExemptScope::FullscreenHdrOnly,
        }
    }
}

/// 仲裁结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExemptVerdict {
    /// 豁免生效：该呈现不套色温。
    Exempted,
    /// 不豁免：套色温。
    Applied,
    /// 豁免本来生效，但用户手动覆盖赢（覆盖期间照常套）。
    ManualOverrideWins,
}

/// 豁免器：范围检查 + 手动覆盖仲裁 + 开关状态机。
pub struct HdrExemptor {
    policy: ExemptPolicy,
    manual_override_active: bool,
    desync_count: u32,
}

impl HdrExemptor {
    /// 用默认策略构造（声明缺失 → 补最小范围，记 [`ExemptCode::ScopeMissing`]）。
    pub fn new() -> Self {
        HdrExemptor {
            policy: ExemptPolicy::default(),
            manual_override_active: false,
            desync_count: 0,
        }
    }

    /// 显式策略构造。
    pub fn with_policy(p: ExemptPolicy) -> Self {
        HdrExemptor {
            policy: p,
            manual_override_active: false,
            desync_count: 0,
        }
    }

    /// 更新策略（范围与开关都在这改）。
    pub fn set_policy(&mut self, p: ExemptPolicy) {
        self.policy = p;
    }

    /// 当前策略。
    pub fn policy(&self) -> &ExemptPolicy {
        &self.policy
    }

    /// 手动覆盖状态入口（F3502 覆盖层同步过来）。
    pub fn set_manual_override(&mut self, active: bool) {
        self.manual_override_active = active;
    }

    /// 仲裁一次 HDR 呈现是否豁免。
    pub fn judge(&self, p: HdrPresentation) -> ExemptVerdict {
        // 手动覆盖赢：覆盖期间一切照常套色温（规则公开第一条）。
        if self.manual_override_active {
            return ExemptVerdict::ManualOverrideWins;
        }
        if !self.policy.enabled || !self.policy.scope.covers(p.fullscreen) {
            return ExemptVerdict::Applied;
        }
        // 范围内还要求：HDR + 播放中。暂停帧不豁免（UI 可读性）。
        if p.hdr && p.playing {
            ExemptVerdict::Exempted
        } else {
            ExemptVerdict::Applied
        }
    }

    /// 越界请求处理：窗口化 HDR 想豁免 → 钳制为不豁免并告警。
    pub fn judge_strict(&mut self, p: HdrPresentation) -> (ExemptVerdict, Option<ExemptCode>) {
        if p.hdr && !p.fullscreen && self.policy.enabled {
            // 明确记录越界尝试：范围是硬边界不是软建议。
            return (ExemptVerdict::Applied, Some(ExemptCode::ScopeExceeded));
        }
        (self.judge(p), None)
    }

    /// 开关重同步（开关失效修复路径）。
    pub fn resync(&mut self, expect_enabled: bool) -> Result<(), ExemptCode> {
        if self.policy.enabled != expect_enabled {
            self.desync_count += 1;
            self.policy.enabled = expect_enabled;
            return Err(ExemptCode::SwitchDesync);
        }
        Ok(())
    }

    /// 失同步累计。
    pub fn desync_count(&self) -> u32 {
        self.desync_count
    }

    /// 规则公开视图。
    pub fn view(&self) -> ExemptView {
        ExemptView {
            enabled: self.policy.enabled,
            scope_text: self.policy.scope.declares(),
            manual_override_active: self.manual_override_active,
            rule: "手动覆盖 > 全屏HDR豁免 > 色温曲线",
        }
    }
}

/// 豁免状态公开视图（读屏播报源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExemptView {
    /// 豁免开关。
    pub enabled: bool,
    /// 范围声明文本。
    pub scope_text: &'static str,
    /// 手动覆盖是否活跃。
    pub manual_override_active: bool,
    /// 仲裁规则（公开）。
    pub rule: &'static str,
}

// ---------------------------------------------------------------------------
// 四、V05 内容保真延续接口
// ---------------------------------------------------------------------------

/// 内容保真仲裁 trait：色温引擎在套色温前问一次。
/// F3501 的 [`super::ver08a_kelvin::PipelineHook`] 实现方持有豁免器，
/// 在 publish 前先问这里。
pub trait FidelityGate {
    /// 该帧是否允许套色温。
    fn allow_tint(&mut self, p: HdrPresentation) -> bool;
}

impl FidelityGate for HdrExemptor {
    fn allow_tint(&mut self, p: HdrPresentation) -> bool {
        !matches!(self.judge(p), ExemptVerdict::Exempted)
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: HdrPresentation = HdrPresentation {
        fullscreen: true,
        hdr: true,
        playing: true,
    };

    #[test]
    fn fullscreen_hdr_exempt() {
        let e = HdrExemptor::new();
        assert_eq!(e.judge(FULL), ExemptVerdict::Exempted);
    }

    #[test]
    fn windowed_hdr_not_exempt() {
        let mut e = HdrExemptor::new();
        let w = HdrPresentation {
            fullscreen: false,
            hdr: true,
            playing: true,
        };
        let (v, code) = e.judge_strict(w);
        assert_eq!(v, ExemptVerdict::Applied);
        assert_eq!(code, Some(ExemptCode::ScopeExceeded));
    }

    #[test]
    fn manual_override_beats_exempt() {
        let mut e = HdrExemptor::new();
        e.set_manual_override(true);
        assert_eq!(e.judge(FULL), ExemptVerdict::ManualOverrideWins);
    }

    #[test]
    fn user_can_disable() {
        let mut e = HdrExemptor::new();
        e.set_policy(ExemptPolicy {
            enabled: false,
            scope: ExemptScope::FullscreenHdrOnly,
        });
        assert_eq!(e.judge(FULL), ExemptVerdict::Applied);
    }

    #[test]
    fn paused_hdr_not_exempt() {
        let e = HdrExemptor::new();
        let paused = HdrPresentation {
            fullscreen: true,
            hdr: true,
            playing: false,
        };
        assert_eq!(e.judge(paused), ExemptVerdict::Applied);
    }
}
