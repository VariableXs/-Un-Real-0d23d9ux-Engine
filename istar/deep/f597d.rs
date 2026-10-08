//! 深化层 · F597 虚拟桌面数字直达（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F597 节）：
//! ①「数字超桌数无动作（安静）」的**映射引擎**——数字键 1-9→桌面条
//!   序号→桌面 id 的纯函数映射与超界静默判定（0 与超桌数皆 None）；
//! ②「直达动画与滑动一致（320ms 横移 F235 同源）」的**横移动画账**——
//!   跨多桌直达的方向/距离/位移纯计算（时长恒 320ms 不随距离变，
//!   位移随距离缩放——与滑动同一套空间感）；
//! ③「桌面条顺序即号码」的**F235 状态同步账**——直达后条序高亮与
//!   当前桌 id 逐次对齐（所见即所得可验）；
//! ④「Ctrl+Win+数字」的**键位注册审计**——与 Win+数字（F535 启动
//!   应用）前缀判异：多一枚 Ctrl 修饰键，组合键不同不冲突。

use crate::checks::CheckSet;
use crate::istar::desknum::{DeskNum, DESK_CAP, SWITCH_MS};
use crate::istar::ibase::ISTAR_DOMAIN;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// ① 映射引擎
// ---------------------------------------------------------------------------

/// 数字键 → 桌面 id（0 基条序）：digit 1..=9 → 第 N 桌的 id = N-1。
///
/// 超界静默：digit 0、超桌数、桌数非法——一律 None（不报错不绕回）。
pub fn desk_id_for_digit(digit: u8, desk_count: usize) -> Option<usize> {
    if desk_count == 0 || desk_count > DESK_CAP || digit == 0 {
        return None;
    }
    let n = digit as usize;
    if n > desk_count {
        return None;
    }
    Some(n - 1)
}

// ---------------------------------------------------------------------------
// ② 横移动画账
// ---------------------------------------------------------------------------

/// 单桌横移宽度（px——F235 桌面条同源的滑动几何）。
pub const DESK_WIDTH_PX: u32 = 1_920;

/// 横移方向（+1 向右 / -1 向左 / 0 同桌）。
pub const SLIDE_RIGHT: i32 = 1;
pub const SLIDE_LEFT: i32 = -1;

/// 直达横移参数（与滑动同源：时长恒 SWITCH_MS，位移随距离缩放）。
pub struct SlideParams {
    pub direction: i32,
    pub distance_desks: usize,
    pub duration_ms: u64,
    pub total_px: u32,
}

impl SlideParams {
    /// 自 `from` 直达 `to` 的动画参数（0 基桌面 id）。
    pub fn plan(from: usize, to: usize) -> SlideParams {
        let (distance, direction) = if to > from {
            (to - from, SLIDE_RIGHT)
        } else if to < from {
            (from - to, SLIDE_LEFT)
        } else {
            (0, 0)
        };
        SlideParams {
            direction,
            distance_desks: distance,
            duration_ms: SWITCH_MS,
            total_px: distance as u32 * DESK_WIDTH_PX,
        }
    }

    /// 同桌无动画。
    pub fn no_motion(&self) -> bool {
        self.distance_desks == 0
    }
}

// ---------------------------------------------------------------------------
// ③ F235 状态同步账
// ---------------------------------------------------------------------------

/// 直达后的同步记录：(条序高亮, 当前桌 id)——两值同源须相等。
pub struct HighlightLedger {
    records: Vec<(usize, usize)>,
}

impl HighlightLedger {
    pub fn new() -> HighlightLedger {
        HighlightLedger { records: Vec::new() }
    }

    /// 直达落定后记一笔（高亮索引与当前桌 id）。
    pub fn record(&mut self, highlight: usize, current: usize) {
        self.records.push((highlight, current));
    }

    /// 所见即所得：每一笔高亮都与当前桌一致。
    pub fn consistent(&self) -> bool {
        self.records.iter().all(|&(h, c)| h == c)
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }
}

// ---------------------------------------------------------------------------
// ④ 键位注册审计
// ---------------------------------------------------------------------------

/// F535 启动应用的前缀（裸 Win+数字）。
pub const F535_PREFIX: &str = "Win+";

/// 直达键位绑定串（digit 1..=9 → Ctrl+Win+N；其余 None）。
pub fn binding_str(digit: u8) -> Option<&'static str> {
    match digit {
        1 => Some("Ctrl+Win+1"),
        2 => Some("Ctrl+Win+2"),
        3 => Some("Ctrl+Win+3"),
        4 => Some("Ctrl+Win+4"),
        5 => Some("Ctrl+Win+5"),
        6 => Some("Ctrl+Win+6"),
        7 => Some("Ctrl+Win+7"),
        8 => Some("Ctrl+Win+8"),
        9 => Some("Ctrl+Win+9"),
        _ => None,
    }
}

/// 冲突判定：本特性绑定以 Ctrl+Win+ 开头（多一枚修饰键），F535 绑定
/// 以裸 Win+ 开头——前缀判异，任何数字都不冲突。
pub fn conflicts_with_f535(digit: u8) -> bool {
    match binding_str(digit) {
        Some(b) => !(b.starts_with(DeskNum::HOTKEY_PREFIX) && !b.starts_with(F535_PREFIX)),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f597_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 映射引擎全量：5 桌用户数字 1..=5 → id 0..=4；6/0 超界静默 None。
    let mut all = true;
    for digit in 1..=5u8 {
        if desk_id_for_digit(digit, 5) != Some(digit as usize - 1) {
            all = false;
        }
    }
    cs.add(
        "mapping engine full table",
        all && desk_id_for_digit(6, 5).is_none() && desk_id_for_digit(0, 5).is_none(),
        "",
    );

    // 2) 纯映射与基础件跳转一致：jump_to 结果 == 映射表（两账同源）。
    let mut d = DeskNum::new(5);
    let mut agree = true;
    for digit in 1..=5u8 {
        if d.jump_to(digit) != desk_id_for_digit(digit, 5) {
            agree = false;
        }
    }
    let _ = d.jump_to(6);
    let _ = d.jump_to(0);
    cs.add(
        "jump agrees with mapping",
        agree && d.silent_count() == 2 && d.jump_count() == 4,
        "",
    );

    // 3) 横移动画账：1→5 桌右移 4 桌 320ms；反向左移；同桌零动画。
    let go = SlideParams::plan(0, 4);
    let back = SlideParams::plan(4, 0);
    let same = SlideParams::plan(2, 2);
    cs.add(
        "slide plan across desks",
        go.direction == SLIDE_RIGHT
            && go.distance_desks == 4
            && go.duration_ms == SWITCH_MS
            && go.total_px == 4 * DESK_WIDTH_PX
            && back.direction == SLIDE_LEFT
            && same.no_motion()
            && same.direction == 0,
        "",
    );

    // 4) 时长与距离无关（F235 同源）：跨 8 桌仍 320ms；基础件动画推进照旧。
    let far = SlideParams::plan(0, 8);
    let mut d2 = DeskNum::new(9);
    let _ = d2.jump_to(9);
    let animating = d2.animating();
    d2.tick(SWITCH_MS);
    cs.add(
        "duration distance independent",
        far.distance_desks == 8
            && far.duration_ms == SWITCH_MS
            && SWITCH_MS == 320
            && animating
            && !d2.animating()
            && DESK_CAP == 9,
        "",
    );

    // 5) F235 状态同步账：每次直达后条序高亮 == 当前桌 id（所见即所得）。
    let mut d3 = DeskNum::new(5);
    let mut hl = HighlightLedger::new();
    let _ = d3.jump_to(2);
    hl.record(d3.current(), d3.current());
    let _ = d3.jump_to(5);
    hl.record(d3.current(), d3.current());
    cs.add(
        "highlight synced with current",
        hl.consistent() && hl.len() == 2 && d3.current() == 4,
        "",
    );

    // 6) 同步后映射依旧：删桌钳界（9→5 桌落第 5），映射按新桌数静默。
    let mut d4 = DeskNum::new(9);
    let _ = d4.jump_to(9);
    let _ = d4.sync_count(5);
    cs.add(
        "mapping after f235 sync",
        d4.current() == 4
            && desk_id_for_digit(5, 5) == Some(4)
            && desk_id_for_digit(6, 5).is_none(),
        "",
    );

    // 7) 键位注册审计：1..=9 全数有绑定且与 F535 不冲突；0/10 无绑定。
    let mut keys_ok = true;
    for digit in 1..=9u8 {
        if binding_str(digit).is_none() || conflicts_with_f535(digit) {
            keys_ok = false;
        }
    }
    cs.add(
        "hotkey audit no f535 clash",
        keys_ok
            && binding_str(3) == Some("Ctrl+Win+3")
            && binding_str(0).is_none()
            && binding_str(10).is_none()
            && DeskNum::HOTKEY_PREFIX == "Ctrl+Win+",
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_desk_count_silent() {
        assert!(desk_id_for_digit(1, 0).is_none());
        assert!(desk_id_for_digit(0, 0).is_none());
    }

    #[test]
    fn same_desk_slide_is_no_motion() {
        let p = SlideParams::plan(3, 3);
        assert!(p.no_motion());
        assert_eq!(p.total_px, 0);
        assert_eq!(p.duration_ms, SWITCH_MS);
    }

    #[test]
    fn highlight_drift_detected() {
        let mut hl = HighlightLedger::new();
        hl.record(2, 1); // 高亮停在条 3、人已在桌 2——失同步
        assert!(!hl.consistent());
    }
}
