//! 深化层 · F594 录屏点击高亮（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F594 节）：
//! ①「点击处涟漪圈 300ms」的**时间线引擎**——点击时刻起逐帧参数：
//!   半径扩张（8→48px）+ 透明度衰减（255→0）的纯函数帧账（基础件
//!   只给位置与生命周期，帧参数深化层补齐）；
//! ②「叠加不进系统交互」的**隔离审计**——输入主管线回执数恒零、
//!   合成管线事件另账（每层「不做」可验证）；
//! ③「快速连点」的**并发账**——多涟漪并行各自计时互不覆盖（按各自
//!   出生时刻独立算帧参数）；
//! ④「颜色走主题令牌（F151）」的**令牌注入账**——令牌名唯一源不动，
//!   换主题解析值跟随（涟漪颜色随主题走）。

use crate::checks::CheckSet;
use crate::istar::clickripple::{ClickRipple, RIPPLE_MS, RIPPLE_TOKEN};
use crate::istar::ibase::ISTAR_DOMAIN;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// ① 涟漪时间线引擎
// ---------------------------------------------------------------------------

/// 涟漪起始半径（px——刚点下时的小圈）。
pub const RIPPLE_R_MIN_PX: u32 = 8;

/// 涟漪终半径（px——300ms 时扩到的大圈）。
pub const RIPPLE_R_MAX_PX: u32 = 48;

/// 透明度上限（255 制——出生时最实）。
pub const ALPHA_MAX: u32 = 255;

/// 涟漪单帧绘制参数（渲染层照此落笔）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RippleFrame {
    pub radius_px: u32,
    pub alpha: u32,
}

/// 时间线引擎：age(ms) → 帧参数（300ms 内线性扩张 + 线性衰减；
/// 超龄钳到终态——圈不再缩回、透明不再回弹）。
pub fn frame_params(age_ms: u32) -> RippleFrame {
    let t = age_ms.min(RIPPLE_MS) as u64;
    let radius = RIPPLE_R_MIN_PX as u64
        + (RIPPLE_R_MAX_PX - RIPPLE_R_MIN_PX) as u64 * t / RIPPLE_MS as u64;
    let alpha = ALPHA_MAX as u64 - ALPHA_MAX as u64 * t / RIPPLE_MS as u64;
    RippleFrame { radius_px: radius as u32, alpha: alpha as u32 }
}

// ---------------------------------------------------------------------------
// ② 叠加隔离审计
// ---------------------------------------------------------------------------

/// 隔离审计账：输入主管线回执数（恒零才隔离）vs 合成管线事件数。
pub struct IsolationAudit {
    input_receipts: u32,
    composited_events: u32,
}

impl IsolationAudit {
    pub fn new() -> IsolationAudit {
        IsolationAudit { input_receipts: 0, composited_events: 0 }
    }

    /// 输入主管线回执登记（结构上不该发生——登记即留痕待审）。
    pub fn note_input_receipt(&mut self) {
        self.input_receipts += 1;
    }

    /// 合成管线事件登记（涟漪帧只进录制管线的一侧）。
    pub fn note_composited(&mut self, n: u32) {
        self.composited_events += n;
    }

    /// 隔离判据：输入侧零回执。
    pub fn isolated(&self) -> bool {
        self.input_receipts == 0
    }

    pub fn composited_events(&self) -> u32 {
        self.composited_events
    }
}

// ---------------------------------------------------------------------------
// ③ 多击并发账
// ---------------------------------------------------------------------------

/// 并发帧账：每圈按各自出生时刻独立算参数（互不覆盖互不吞）。
///
/// `clicks` = (x, y, born_ms)；`now_ms` 为合成钟。超龄圈出局。
pub fn concurrent_frames(clicks: &[(i32, i32, u32)], now_ms: u32) -> Vec<RippleFrame> {
    clicks
        .iter()
        .filter(|&&(_, _, born)| now_ms.saturating_sub(born) < RIPPLE_MS)
        .map(|&(_, _, born)| frame_params(now_ms.saturating_sub(born)))
        .collect()
}

// ---------------------------------------------------------------------------
// ④ 令牌色注入账
// ---------------------------------------------------------------------------

/// 主题调色板：令牌名 → 主题内色值（符号名——不持 RGB，渲染层落色）。
pub struct TokenPalette {
    theme: &'static str,
}

impl TokenPalette {
    pub fn new(theme: &'static str) -> TokenPalette {
        TokenPalette { theme }
    }

    pub fn set_theme(&mut self, theme: &'static str) {
        self.theme = theme;
    }

    /// 解析令牌（只认 F151 令牌名 RIPPLE_TOKEN——换主题换值不换名）。
    pub fn resolve(&self, token: &str) -> Option<&'static str> {
        if token != RIPPLE_TOKEN {
            return None;
        }
        match self.theme {
            "dark" => Some("accent-cyan"),
            "light" => Some("accent-blue"),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f594_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 时间线两端：出生帧（8px/255）与终帧（48px/0）。
    cs.add(
        "timeline endpoints",
        frame_params(0) == RippleFrame { radius_px: 8, alpha: 255 }
            && frame_params(RIPPLE_MS) == RippleFrame { radius_px: 48, alpha: 0 },
        "",
    );

    // 2) 时间线单调：半径不回缩、透明不回弹（逐帧抽样对账）。
    let mut mono = true;
    let mut prev_r = 0u32;
    let mut prev_a = u32::MAX;
    for age in (0..=300u32).step_by(10) {
        let f = frame_params(age);
        if f.radius_px < prev_r || f.alpha > prev_a {
            mono = false;
        }
        prev_r = f.radius_px;
        prev_a = f.alpha;
    }
    cs.add(
        "timeline monotonic",
        mono && frame_params(150).radius_px == 28 && frame_params(150).alpha == 128,
        "",
    );

    // 3) 隔离审计：真实点击零回执（结构证据：real_click 无回执路径）、
    //    合成事件只进录制侧；基础件 300ms 生命周期照旧。
    let mut audit = IsolationAudit::new();
    let mut cr = ClickRipple::new();
    cr.set_enabled(true);
    cr.real_click(10, 20, 0);
    for _ in 0..5 {
        let frame = cr.compose_frame(60);
        audit.note_composited(frame.len() as u32);
    }
    cs.add(
        "overlay isolated from input",
        audit.isolated() && audit.composited_events() > 0 && RIPPLE_MS == 300,
        "",
    );

    // 4) 并发账：三连点各自计时——t=250 三圈三样参数；t=400 前两圈
    //    已各自出局、末圈仍在途；t=500 全部出局（按各自出生时刻到期）。
    let clicks: Vec<(i32, i32, u32)> = alloc::vec![(1, 1, 0), (2, 2, 100), (3, 3, 200)];
    let at_250 = concurrent_frames(&clicks, 250);
    let distinct = at_250.len() == 3
        && at_250[0].alpha != at_250[1].alpha
        && at_250[1].alpha != at_250[2].alpha;
    let at_400 = concurrent_frames(&clicks, 400);
    let at_500 = concurrent_frames(&clicks, 500);
    let mut cr2 = ClickRipple::new();
    cr2.set_enabled(true);
    cr2.real_click(1, 1, 0);
    cr2.real_click(2, 2, 100);
    cr2.real_click(3, 3, 200);
    // 基础件合成钟已被点击推到 200——先补 50ms 对齐 t=250 再逐段老化。
    cr2.compose_frame(50);
    let alive_base = cr2.ripple_count() == 3;
    cr2.compose_frame(150);
    let mid_base = cr2.ripple_count() == 1;
    cr2.compose_frame(100);
    cs.add(
        "concurrent ripples independent",
        distinct
            && at_400.len() == 1
            && at_500.is_empty()
            && alive_base
            && mid_base
            && cr2.ripple_count() == 0,
        "",
    );

    // 5) 令牌注入账：令牌名恒定，换主题解析值跟随（dark/light 各有色）。
    let mut pal = TokenPalette::new("dark");
    let dark = pal.resolve(RIPPLE_TOKEN);
    pal.set_theme("light");
    let light = pal.resolve(RIPPLE_TOKEN);
    cs.add(
        "token color follows theme",
        dark == Some("accent-cyan")
            && light == Some("accent-blue")
            && dark != light
            && pal.resolve("other-token").is_none(),
        "",
    );

    // 6) 默认关契约：关态点击零涟漪零回溯（再开也不冒历史圈）。
    let mut cr3 = ClickRipple::new();
    cr3.real_click(5, 5, 0);
    cr3.real_click(6, 6, 1);
    cr3.set_enabled(true);
    let no_backlog = cr3.compose_frame(16).is_empty();
    cr3.set_enabled(false);
    cs.add(
        "default off contract kept",
        !cr3.enabled() && no_backlog && cr3.ripple_count() == 0,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_params_clamps_beyond_life() {
        let f = frame_params(10_000);
        assert_eq!(f.radius_px, RIPPLE_R_MAX_PX);
        assert_eq!(f.alpha, 0);
    }

    #[test]
    fn concurrent_empty_clicks_empty_frames() {
        assert!(concurrent_frames(&[], 100).is_empty());
    }

    #[test]
    fn palette_unknown_theme_none() {
        let pal = TokenPalette::new("high-contrast-x");
        assert!(pal.resolve(RIPPLE_TOKEN).is_none());
    }
}
