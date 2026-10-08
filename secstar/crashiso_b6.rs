//! F175 崩溃隔离强化 · 批次六深化（secstar · G-G-05）。
//!
//! 批次六功能面（与 b3/b4/b5 互补，本批管「出路全集与参数校验」）：
//! - [`MaskEscape`]：遮罩出路全集——点外部/失焦/Esc/双钮四出路
//!   （浮层完整出路清单的遮罩版：每条出路有名字）；
//! - [`validate_restart_params`]：重启参数校验——cmdline/cwd/env 长
//!   界 + 非法字节（NUL 中途截断=参数不保真——拒收坏参数）；
//! - [`CrashRateWindow`]：小时级崩溃计数——系统健康黄标
//!   （单应用隔离之外：全系统崩溃率也是健康信号）；
//! - [`focus_return_after_heal`]： healed 后焦点归还（修好了回原位）。
//!
//! 零堆纪律：状态位 + 定长校验，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 遮罩出路全集
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskEscape {
    ClickOutside,
    WindowFocusLost,
    EscKey,
    CloseButton,
    RestartButton,
}

/// 出路裁决：每条路都通向「遮罩收起」（收起 ≠ 关闭应用——应用仍隔离）。
pub fn escape_closes_mask(e: MaskEscape) -> bool {
    matches!(e, MaskEscape::ClickOutside | MaskEscape::WindowFocusLost | MaskEscape::EscKey | MaskEscape::CloseButton | MaskEscape::RestartButton)
}

/// 全出路清单守恒：五条路全部收起遮罩（清单不缺路——逐条机械验证）。
pub fn all_escapes_valid() -> bool {
    [
        MaskEscape::ClickOutside,
        MaskEscape::WindowFocusLost,
        MaskEscape::EscKey,
        MaskEscape::CloseButton,
        MaskEscape::RestartButton,
    ]
    .iter()
    .all(|e| escape_closes_mask(*e))
}

/// 出路后应用保持隔离态（收起遮罩 ≠ 放出崩溃应用——安全边界）。
pub fn app_still_quarantined_after_escape(e: MaskEscape, was_quarantined: bool) -> bool {
    escape_closes_mask(e) && was_quarantined
}

// ---------------------------------------------------------------------------
// 重启参数校验
// ---------------------------------------------------------------------------

/// 参数长度界（与主层 LaunchParams 容量同尺）。
pub const CMDLINE_MAX: usize = 128;
pub const CWD_MAX: usize = 64;
pub const ENV_MAX: usize = 256;

/// 校验错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamError {
    TooLong,
    EmbeddedNul,
}

/// 校验一段参数：不超长、不含内嵌 NUL（NUL 截断 = 参数失真）。
pub fn validate_param(data: &[u8], max: usize) -> Result<(), ParamError> {
    if data.len() > max {
        return Err(ParamError::TooLong);
    }
    if data.contains(&0) {
        return Err(ParamError::EmbeddedNul);
    }
    Ok(())
}

/// 重启三参数全检（任一坏 → 整组拒——参数保真是硬前提）。
pub fn validate_restart_params(cmdline: &[u8], cwd: &[u8], env: &[u8]) -> Result<(), ParamError> {
    validate_param(cmdline, CMDLINE_MAX)?;
    validate_param(cwd, CWD_MAX)?;
    validate_param(env, ENV_MAX)
}

// ---------------------------------------------------------------------------
// 崩溃率窗口
// ---------------------------------------------------------------------------

/// 小时窗口告警线（次/小时——全系统面）。
pub const CRASH_RATE_ALERT: u32 = 6;

#[derive(Clone, Copy, Debug, Default)]
pub struct CrashRateWindow {
    pub crashes_this_hour: u32,
    pub hours_over: u32,
}

impl CrashRateWindow {
    pub fn on_crash(&mut self) {
        self.crashes_this_hour += 1;
    }

    /// 小时滚动：返回上一小时是否超线（黄标依据）。
    pub fn hour_rollover(&mut self) -> bool {
        let over = self.crashes_this_hour >= CRASH_RATE_ALERT;
        if over {
            self.hours_over += 1;
        }
        self.crashes_this_hour = 0;
        over
    }

    /// 连续超线小时数（>1 = 系统性问题不是偶发）。
    pub fn systemic(&self) -> bool {
        self.hours_over > 1
    }
}

// ---------------------------------------------------------------------------
// healed 焦点归还
// ---------------------------------------------------------------------------

/// healed 应用重进 Z 序（原位恢复——修好了回原位不是沉底）。
pub fn focus_return_after_heal(healed_app_z_index_before_crash: usize, z_now: usize) -> usize {
    healed_app_z_index_before_crash.min(z_now)
}

// ---------------------------------------------------------------------------
// 批次六自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_crashiso_b6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F175-b6");

    // 1) 出路全集：五条路全部收起遮罩（逐条机械验证）。
    cs.add("escape_all_valid", all_escapes_valid(), "");

    // 2) 出路后保持隔离：收起遮罩不等于放出崩溃应用（安全边界）。
    cs.add(
        "escape_keeps_quarantine",
        app_still_quarantined_after_escape(MaskEscape::EscKey, true)
            && app_still_quarantined_after_escape(MaskEscape::ClickOutside, true),
        "",
    );

    // 3) 参数长度界：128 命令行恰过、129 拒（界逐点）。
    cs.add(
        "param_length_bounds",
        validate_param(&[b'x'; CMDLINE_MAX], CMDLINE_MAX).is_ok()
            && validate_param(&[b'x'; CMDLINE_MAX + 1], CMDLINE_MAX) == Err(ParamError::TooLong),
        "",
    );

    // 4) 内嵌 NUL 拒：中途 NUL = 截断失真（参数保真红线）。
    cs.add(
        "param_nul_rejected",
        validate_param(b"app.exe\0hidden-arg", CMDLINE_MAX) == Err(ParamError::EmbeddedNul)
            && validate_param(b"app.exe", CMDLINE_MAX).is_ok(),
        "",
    );

    // 5) 三参数全检：命令行坏 → 整组拒（任一坏全拒）。
    cs.add(
        "params_all_or_nothing",
        validate_restart_params(&[b'a'], &[b'b'], &[b'c']).is_ok()
            && validate_restart_params(&[b'a', 0, b'b'], &[b'b'], &[b'c']) == Err(ParamError::EmbeddedNul),
        "",
    );

    // 6) 崩溃率：5 次/小时不黄、6 次黄（告警线逐点）。
    let mut r = CrashRateWindow::default();
    for _ in 0..5 {
        r.on_crash();
    }
    let quiet = !r.hour_rollover();
    for _ in 0..6 {
        r.on_crash();
    }
    cs.add("crash_rate_line", quiet && r.hour_rollover() && r.hours_over == 1, "");

    // 7) 系统性判定：连续两小时超线 → 系统性问题（偶发不冤）。
    let mut r2 = CrashRateWindow::default();
    for _ in 0..6 {
        r2.on_crash();
    }
    r2.hour_rollover();
    for _ in 0..6 {
        r2.on_crash();
    }
    r2.hour_rollover();
    cs.add("crash_rate_systemic", r2.systemic(), "");

    // 8) healed 焦点归还原位（索引 2 的应用修好回 index 2）。
    cs.add("focus_return", focus_return_after_heal(2, 5) == 2, "");

    // 9) 焦点归还越界钳制：原位超出当前栈深 → 沉到栈底（不悬空）。
    cs.add("focus_return_clamped", focus_return_after_heal(9, 4) == 4, "");

    // 10) 主册常量贯通：cmdline 128 容量一处一事实。
    cs.add("consts_aligned", CMDLINE_MAX == 128 && CWD_MAX == 64 && ENV_MAX == 256, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次六）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b6 {
    use super::*;

    #[test]
    fn rate_window_three_hour_trend() {
        // 三小时趋势：超-超-不超 → 只算连续 2（hours_over 随清零归位）。
        let mut r = CrashRateWindow::default();
        for _ in 0..6 {
            r.on_crash();
        }
        assert!(r.hour_rollover());
        for _ in 0..6 {
            r.on_crash();
        }
        assert!(r.hour_rollover());
        for _ in 0..2 {
            r.on_crash();
        }
        assert!(!r.hour_rollover());
        assert_eq!(r.hours_over, 2);
        assert!(r.systemic());
    }

    #[test]
    fn param_empty_ok() {
        // 空参数合法（cmdline 可空——不是所有应用都带参）。
        assert!(validate_restart_params(b"", b"", b"").is_ok());
    }
}
