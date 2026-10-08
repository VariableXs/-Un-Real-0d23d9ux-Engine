//! 深化层 · F565 登录问候（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F565 节）：
//! ①「问候从不过度（一句封顶）」的**同日一次账**——同一自然天重复
//!   解锁不再问候（基础层只有 showing 防重叠，跨次解锁会重复刷）；
//! ②「淡入淡出 1.5s」的**对称曲线**——主册说淡入淡出，基础层是线性
//!   淡入后即斩断；深化层给出对称曲线（前 750ms 淡入 0→1000‰、后
//!   750ms 淡出 1000→0‰），偏差在注释里如实声明；
//! ③「星语（每日一句，30 句轮换）」的**日序对齐口**——按解锁日序号
//!   取句（跨天换句、同天同句），轮换语义可复现。

use crate::checks::CheckSet;
use crate::istar::greet::{Greeter, FADE_MS, STAR_WORD_PERIOD_DAYS, STAR_WORDS};
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 同日一次账
// ---------------------------------------------------------------------------

/// 同日一次账：解锁日序号（unix_day）去重。
pub struct DayOnceLedger {
    last_day: Option<u64>,
    shown_today: bool,
    suppressed: u32,
}

impl DayOnceLedger {
    pub fn new() -> DayOnceLedger {
        DayOnceLedger { last_day: None, shown_today: false, suppressed: 0 }
    }

    /// 解锁请求：当天第一次放行，当天后续一律抑制（过度防护的账面）。
    pub fn on_unlock(&mut self, unix_day: u64) -> bool {
        match self.last_day {
            Some(d) if d == unix_day && self.shown_today => {
                self.suppressed += 1;
                false
            }
            _ => {
                self.last_day = Some(unix_day);
                self.shown_today = true;
                true
            }
        }
    }

    /// 跨天（新的一天解锁重新放行）。
    pub fn suppressed_total(&self) -> u32 {
        self.suppressed
    }
}

impl Default for DayOnceLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 对称淡入淡出曲线
// ---------------------------------------------------------------------------

/// 对称曲线：前半程淡入（0→1000‰）、后半程淡出（1000→0‰），1.5s 走完。
///
/// 主册「淡入淡出 1.5s」的按字执行；基础层线性淡入后斩断的偏差在此
/// 显性登记（渲染层应取本函数为唯一曲线源）。
pub fn fade_in_out_permille(elapsed_ms: u64) -> Option<u32> {
    if elapsed_ms >= FADE_MS {
        return None; // 走完自动消失
    }
    let half = FADE_MS / 2;
    if elapsed_ms <= half {
        Some((elapsed_ms * 1000 / half) as u32)
    } else {
        Some(((FADE_MS - elapsed_ms) * 1000 / half) as u32)
    }
}

// ---------------------------------------------------------------------------
// 星语日序对齐
// ---------------------------------------------------------------------------

/// 按解锁日序号取星语（unix_day % 30 —— 跨天换句、同天同句、可复现）。
pub fn star_word_for_day(unix_day: u64) -> &'static str {
    STAR_WORDS[(unix_day % STAR_WORD_PERIOD_DAYS) as usize]
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f565_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 同日一次：当天第二次解锁被抑制且留痕。
    let mut d = DayOnceLedger::new();
    let a = d.on_unlock(20_600);
    let b = d.on_unlock(20_600);
    let c = d.on_unlock(20_601);
    cs.add(
        "same day shown once",
        a && !b && c && d.suppressed_total() == 1,
        "",
    );

    // 2) 对称曲线：750ms 峰值 1000‰、两端 0、整体对称、走完消失。
    let up = fade_in_out_permille(375).unwrap();
    let peak = fade_in_out_permille(750).unwrap();
    let down = fade_in_out_permille(1_125).unwrap();
    cs.add(
        "fade symmetric curve",
        up == 500 && peak == 1000 && down == 500 && fade_in_out_permille(0) == Some(0)
            && fade_in_out_permille(FADE_MS).is_none(),
        "",
    );

    // 2b) 峰前一步：749ms → 998‰（整除截断——曲线单调不跳变）。
    cs.add("fade pre-peak stepwise", fade_in_out_permille(749) == Some(998), "");

    // 3) 基础层联动：Greeter 门关着时问候不出（可关判据不被深化破坏）。
    let mut g = Greeter::new();
    g.set_enabled(false);
    cs.add("disabled greeter stays quiet", !g.on_unlock(1_000), "");

    // 4) 星语日序：同天同句、跨天换句、30 天一轮回到起点。
    let w0 = star_word_for_day(100);
    let w0b = star_word_for_day(100);
    let w1 = star_word_for_day(101);
    let w_wrap = star_word_for_day(100 + STAR_WORD_PERIOD_DAYS);
    cs.add(
        "star word daily rotation",
        w0 == w0b && w0 != w1 && w_wrap == w0 && STAR_WORDS.len() == 30,
        "",
    );

    // 5) 星语池无空句（30 句池的合同：每句可渲染）。
    cs.add("star pool no empty lines", STAR_WORDS.iter().all(|s| !s.is_empty()), "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_peak_exactly_half() {
        assert_eq!(fade_in_out_permille(FADE_MS / 2), Some(1000));
        assert_eq!(fade_in_out_permille(FADE_MS / 2 - 1), Some(998));
    }

    #[test]
    fn day_once_new_day_resets() {
        let mut d = DayOnceLedger::new();
        assert!(d.on_unlock(5));
        assert!(!d.on_unlock(5));
        assert!(d.on_unlock(6));
    }
}
