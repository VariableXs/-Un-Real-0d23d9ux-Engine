//! F498 高 DPI 模糊修复提示（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **模糊检测判据（非 aware 应用识别）；一键应用与重绘；不再提示；与 F028
//! 三态底层联动；提示一次性记账。**
//!
//! 功能定义（主册批次三）：旧应用在高分屏发糊时（非 DPI aware 应用被系统
//! 拉伸）——系统检测到模糊场景弹一次性提示条（「此应用在高分屏下可能模糊
//! ——尝试高 DPI 优化？」一键应用）；应用后立即重绘验证；「不再为此应用
//! 提示」选项；每个应用只烦你一次。
//!
//! 零堆纪律：定长记账表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// F028 三态（底层联动——本项是三态的用户侧入口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DpiAwareness {
    Unaware,
    System,
    PerMonitor,
}

/// 拉伸倍率超过此值判「模糊场景」（150% 缩放下非 aware 必糊——主册场景）。
pub const BLUR_SCALE_PERMILLE: u16 = 150;

/// 提示条文案（一次性、可操作）。
pub const PROMPT_TEXT: &str = "此应用在高分屏下可能模糊——尝试高 DPI 优化？";

/// DPI 修复管理器。
pub struct DpiFix {
    /// 每应用提示记账（提示一次性——每个应用只烦你一次）。
    prompted: [Option<u64>; 32],
    n: usize,
    /// 永不再提示名单（用户选择——诚实尊重）。
    silenced: [Option<u64>; 32],
    silenced_n: usize,
}

fn app_key(name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 模糊检测判据（主册：非 aware 应用识别——感知态 × 缩放倍率；150% 即糊）。
pub fn blur_detected(aware: DpiAwareness, scale_permille: u16) -> bool {
    aware == DpiAwareness::Unaware && scale_permille >= BLUR_SCALE_PERMILLE
}

impl DpiFix {
    pub const fn new() -> Self {
        DpiFix { prompted: [None; 32], n: 0, silenced: [None; 32], silenced_n: 0 }
    }

    fn in_list(list: &[Option<u64>; 32], n: usize, k: u64) -> bool {
        (0..n).any(|i| list[i] == Some(k))
    }

    /// 是否弹提示（模糊 + 从未提示过 + 未被静音——一次性记账）。
    pub fn should_prompt(&mut self, app: &str, aware: DpiAwareness, scale_permille: u16) -> bool {
        if !blur_detected(aware, scale_permille) {
            return false;
        }
        let k = app_key(app);
        if Self::in_list(&self.silenced, self.silenced_n, k) || Self::in_list(&self.prompted, self.n, k) {
            return false;
        }
        if self.n < 32 {
            self.prompted[self.n] = Some(k);
            self.n += 1;
        }
        true
    }

    /// 「不再为此应用提示」（用户选择记账）。
    pub fn silence(&mut self, app: &str) -> bool {
        let k = app_key(app);
        if Self::in_list(&self.silenced, self.silenced_n, k) {
            return true; // 幂等
        }
        if self.silenced_n >= 32 {
            return false;
        }
        self.silenced[self.silenced_n] = Some(k);
        self.silenced_n += 1;
        true
    }

    /// 一键应用（F028 三态底层联动：Unaware → PerMonitor 优化）。
    /// 返回应用后的感知态（立即重绘由调用层触发）。
    pub fn apply_fix(from: DpiAwareness) -> Option<DpiAwareness> {
        match from {
            DpiAwareness::Unaware => Some(DpiAwareness::PerMonitor),
            _ => None, // 非 unaware 应用不糊——无修复可做（诚实）
        }
    }

    /// 重绘验证（应用后立即重绘——对比度/清晰度判据模拟：应用后不再判糊）。
    pub fn redraw_verify(after: DpiAwareness, scale_permille: u16) -> bool {
        !blur_detected(after, scale_permille)
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_dpifix_checks() -> CheckSet {
    let mut cs = CheckSet::new("F498-dpifix");
    // 1) 模糊检测判据（非 aware + 150% 缩放 → 糊；aware 不糊）。
    cs.add("blur_unaware_150", blur_detected(DpiAwareness::Unaware, 150), "");
    cs.add("no_blur_aware", !blur_detected(DpiAwareness::System, 200) && !blur_detected(DpiAwareness::PerMonitor, 200), "");
    cs.add("no_blur_100", !blur_detected(DpiAwareness::Unaware, 100), "");
    // 2) 提示一次性记账（每个应用只烦你一次）。
    let mut d = DpiFix::new();
    cs.add("prompt_once", d.should_prompt("old-app", DpiAwareness::Unaware, 150), "");
    cs.add("prompt_never_twice", !d.should_prompt("old-app", DpiAwareness::Unaware, 150), "");
    // 3) 不再提示（静音名单）。
    cs.add("silence_works", d.silence("another-app") && !d.should_prompt("another-app", DpiAwareness::Unaware, 200), "");
    // 4) 一键应用与重绘（Unaware → PerMonitor 后 200% 不糊）。
    cs.add("apply_fix", DpiFix::apply_fix(DpiAwareness::Unaware) == Some(DpiAwareness::PerMonitor), "");
    cs.add("redraw_verify", DpiFix::redraw_verify(DpiAwareness::PerMonitor, 200), "");
    cs.add("fix_honest_noop", DpiFix::apply_fix(DpiAwareness::System).is_none(), "");
    // 5) 三态底层联动（枚举与 F028 同构）。
    cs.add("three_states", DpiAwareness::Unaware as u8 == 0 && DpiAwareness::System as u8 == 1 && DpiAwareness::PerMonitor as u8 == 2, "");
    // 6) 文案在册。
    cs.add("prompt_text", PROMPT_TEXT.contains("高分屏") && PROMPT_TEXT.ends_with("？"), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_exactly_once_per_app() {
        let mut d = DpiFix::new();
        assert!(d.should_prompt("legacy", DpiAwareness::Unaware, 200));
        assert!(!d.should_prompt("legacy", DpiAwareness::Unaware, 200));
        // 换应用独立记账。
        assert!(d.should_prompt("legacy2", DpiAwareness::Unaware, 150));
    }

    #[test]
    fn silence_preempts_prompt_forever() {
        let mut d = DpiFix::new();
        d.silence("sticky");
        for _ in 0..3 {
            assert!(!d.should_prompt("sticky", DpiAwareness::Unaware, 300));
        }
    }

    #[test]
    fn fix_chain_end_to_end() {
        // 检测 → 提示 → 一键应用 → 重绘验证通过（用户侧完整链）。
        let mut d = DpiFix::new();
        assert!(d.should_prompt("app", DpiAwareness::Unaware, 150));
        let fixed = DpiFix::apply_fix(DpiAwareness::Unaware).unwrap();
        assert!(DpiFix::redraw_verify(fixed, 150));
    }
}
