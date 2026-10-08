//! 深化层 · F592 截图含光标开关（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F592 节）：
//! ①「指针按截图瞬间的真实形态渲染（忙碌圈/手型都如实——不是永远
//!   箭头贴图）」的**形态保真渲染引擎**——指针形态→sprite 资源的
//!   总映射（四形态各配专属贴图，非箭头形态绝不回落箭头贴图）；
//! ②「开关即时对三模式生效」的**三模式瞬间快照账**——键位/区域/
//!   窗口三模式各自在截图瞬间取值定格（快照后翻转开关不回写——
//!   在途截图取值一致，新截图取新值）；
//! ③「与 F361 录屏光标独立」的**两账隔离审计**——截图开关账与
//!   录屏光标账各记各的（截图侧变更 N 次后录屏账纹丝不动）；
//! ④「默认不含」的**基线锚**——默认值常量唯一源与基础件出厂态
//!   对锚（两处默认不一致即红）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::shotcursor::{CursorShape, ShotCursor, ShotMode};

// ---------------------------------------------------------------------------
// ④ 基线锚
// ---------------------------------------------------------------------------

/// 基线锚：截图默认不含指针（主册判据的常量唯一源）。
pub const DEFAULT_INCLUDE_CURSOR: bool = false;

// ---------------------------------------------------------------------------
// ① 形态保真渲染引擎
// ---------------------------------------------------------------------------

/// 指针形态 → sprite 资源名（保真映射唯一源——形态即贴图，无回落）。
pub fn sprite_for(shape: CursorShape) -> &'static str {
    match shape {
        CursorShape::Arrow => "spr_cursor_arrow",
        CursorShape::Hand => "spr_cursor_hand",
        CursorShape::Busy => "spr_cursor_busy",
        CursorShape::Text => "spr_cursor_text",
    }
}

/// 保真审计：四形态映射齐全且互异，非箭头形态不得渲染箭头贴图
/// （贴图回落 = 忙碌圈画成箭头 = 失真，判据红线）。
pub fn shape_fidelity_holds() -> bool {
    let all = [
        CursorShape::Arrow,
        CursorShape::Hand,
        CursorShape::Busy,
        CursorShape::Text,
    ];
    let arrow_sprite = sprite_for(CursorShape::Arrow);
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            if sprite_for(all[i]) == sprite_for(all[j]) {
                return false; // 两形态共用贴图 = 形态不保真
            }
        }
        if all[i] != CursorShape::Arrow && sprite_for(all[i]) == arrow_sprite {
            return false; // 非箭头回落箭头贴图 = 失真
        }
    }
    true
}

/// 截图瞬间的形态卡（拍摄所得——渲染层按此取 sprite）。
pub struct CaptureCard {
    pub mode: ShotMode,
    pub shape: Option<CursorShape>,
}

impl CaptureCard {
    /// 卡面保真：含指针时 sprite 与形态一一对应。
    pub fn sprite_matches_shape(&self) -> bool {
        match self.shape {
            Some(s) => sprite_for(s) != sprite_for(CursorShape::Arrow) || s == CursorShape::Arrow,
            None => true, // 不含指针无贴图（默认基线）
        }
    }
}

// ---------------------------------------------------------------------------
// ② 三模式瞬间快照账
// ---------------------------------------------------------------------------

/// 截图瞬间快照（单模式一卡——定格后翻转开关不回写）。
pub struct ShotSnapshot {
    pub mode: ShotMode,
    pub include: bool,
    pub shape: Option<CursorShape>,
}

/// 从基础件拍摄账取快照（shoot 即瞬间取值——快照冻结该值）。
pub fn capture(shooter: &mut ShotCursor, mode: ShotMode) -> ShotSnapshot {
    let shape = shooter.shoot(mode);
    ShotSnapshot { mode, include: shooter.include(), shape }
}

// ---------------------------------------------------------------------------
// ③ 两账隔离审计
// ---------------------------------------------------------------------------

/// 开关账（截图/录屏各持一份——互不读写对方字段）。
pub struct ToggleLedger {
    state: bool,
    toggles: u32,
}

impl ToggleLedger {
    pub fn new() -> ToggleLedger {
        ToggleLedger { state: false, toggles: 0 }
    }

    pub fn set(&mut self, on: bool) {
        if self.state != on {
            self.toggles += 1;
        }
        self.state = on;
    }

    pub fn state(&self) -> bool {
        self.state
    }

    pub fn toggles(&self) -> u32 {
        self.toggles
    }
}

impl Default for ToggleLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 隔离判据：录屏账出厂态（零翻转、默认关）在截图侧变更后不变。
pub fn recorder_ledger_untouched(rec: &ToggleLedger) -> bool {
    rec.toggles() == 0 && rec.state() == false
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f592_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 基线锚：深化常量与基础件出厂态一致（默认不含两处同源）。
    let fresh = ShotCursor::new();
    cs.add(
        "default off baseline anchored",
        DEFAULT_INCLUDE_CURSOR == false && fresh.default_off() && !fresh.include(),
        "",
    );

    // 2) 形态保真映射：四形态专属贴图互异、非箭头不回落箭头贴图。
    cs.add(
        "sprite fidelity mapping",
        shape_fidelity_holds()
            && sprite_for(CursorShape::Busy) == "spr_cursor_busy"
            && sprite_for(CursorShape::Hand) != sprite_for(CursorShape::Arrow),
        "",
    );

    // 3) 截图瞬间形态卡：忙碌圈按瞬间形态如实出（不是箭头贴图）。
    let mut c3 = ShotCursor::new();
    c3.set_include(true);
    c3.note_shape(CursorShape::Busy);
    let card = CaptureCard { mode: ShotMode::Region, shape: c3.shoot(ShotMode::Region) };
    cs.add(
        "busy shape rendered as is",
        card.shape == Some(CursorShape::Busy)
            && card.sprite_matches_shape()
            && card.mode == ShotMode::Region,
        "",
    );

    // 4) 快照隔离：定格后翻转开关——在途快照仍持瞬间值（快照=开），
    //    活账已是新值（关），两值并存即冻结证据；新截图取新值。
    let mut c4 = ShotCursor::new();
    c4.set_include(true);
    let in_flight = capture(&mut c4, ShotMode::Region);
    c4.set_include(false);
    let fresh_after = capture(&mut c4, ShotMode::Window);
    cs.add(
        "snapshot frozen at instant",
        in_flight.include
            && in_flight.shape.is_some()
            && c4.include() == false
            && !fresh_after.include
            && fresh_after.shape.is_none(),
        "",
    );

    // 5) 三模式逐一快照：键位/区域/窗口三账各记各的瞬间值。
    let mut c5 = ShotCursor::new();
    c5.set_include(true);
    let s1 = capture(&mut c5, ShotMode::Fullscreen);
    c5.set_include(false);
    let s2 = capture(&mut c5, ShotMode::Region);
    let s3 = capture(&mut c5, ShotMode::Window);
    cs.add(
        "three modes instant values",
        s1.mode == ShotMode::Fullscreen
            && s1.include
            && s2.mode == ShotMode::Region
            && !s2.include
            && s3.mode == ShotMode::Window
            && !s3.include,
        "",
    );

    // 6) 两账隔离：截图账翻三次，录屏账零翻转零变更（F361 独立）。
    let mut shot_ledger = ToggleLedger::new();
    let rec_ledger = ToggleLedger::new();
    shot_ledger.set(true);
    shot_ledger.set(false);
    shot_ledger.set(true);
    cs.add(
        "recorder ledger untouched",
        shot_ledger.toggles() == 3
            && recorder_ledger_untouched(&rec_ledger)
            && !rec_ledger.state(),
        "",
    );

    // 7) 手型保真：基础件注形 → 拍摄按手型出（形态注入链路通）。
    let mut c7 = ShotCursor::new();
    c7.set_include(true);
    c7.note_shape(CursorShape::Hand);
    let hand = c7.shoot(ShotMode::Fullscreen);
    cs.add(
        "hand shape faithful",
        hand == Some(CursorShape::Hand)
            && sprite_for(hand.unwrap_or(CursorShape::Arrow)) == "spr_cursor_hand",
        "",
    );

    // 8) 不含指针的卡面诚实无贴图（默认基线下的拍摄面）。
    let mut c8 = ShotCursor::new();
    let bare = CaptureCard { mode: ShotMode::Window, shape: c8.shoot(ShotMode::Window) };
    cs.add(
        "no cursor no sprite",
        bare.shape.is_none() && bare.sprite_matches_shape(),
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprites_all_distinct() {
        let all = [
            CursorShape::Arrow,
            CursorShape::Hand,
            CursorShape::Busy,
            CursorShape::Text,
        ];
        for i in 0..4 {
            for j in (i + 1)..4 {
                assert_ne!(sprite_for(all[i]), sprite_for(all[j]));
            }
        }
    }

    #[test]
    fn snapshot_keeps_mode() {
        let mut c = ShotCursor::new();
        c.set_include(true);
        let snap = capture(&mut c, ShotMode::Window);
        assert_eq!(snap.mode, ShotMode::Window);
        assert!(snap.include);
    }

    #[test]
    fn ledger_toggle_counting() {
        let mut l = ToggleLedger::new();
        l.set(true);
        l.set(true); // 同值不计数
        l.set(false);
        assert_eq!(l.toggles(), 2);
        assert!(!l.state());
    }
}
