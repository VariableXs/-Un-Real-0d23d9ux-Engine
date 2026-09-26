
// ---------------------------------------------------------------------------
// F018 · 深化批次六：落点效果修饰键语义（Ctrl/Shift → Copy/Move/Link）
//
// 主册依据（G-A-18【功能定义】）：「OLE 拖放（DoDragDrop/IDropSource/
// IDropTarget 语义）」——Windows 资源管理器修饰键语义：无修饰 = 缺省
// （同卷 Move/跨卷 Copy，由既有 drop_route 裁决）；Ctrl = Copy；Shift =
// Move；Ctrl+Shift = Link。
// ---------------------------------------------------------------------------

/// 修饰键位（MK_ 常量——winuser.h 钉值）。
pub const MK_SHIFT: u32 = 0x0004;
pub const MK_CONTROL: u32 = 0x0008;

/// 修饰键 → 显式落点效果（None = 无显式修饰，交既有 drop_route 裁决）。
pub fn effect_from_modifiers(mk: u32) -> Option<DropEffect> {
    let ctrl = mk & MK_CONTROL != 0;
    let shift = mk & MK_SHIFT != 0;
    match (ctrl, shift) {
        (true, true) => Some(DropEffect::Link),
        (true, false) => Some(DropEffect::Copy),
        (false, true) => Some(DropEffect::Move),
        (false, false) => None,
    }
}

/// F018 深化批次六自检。
pub fn run_dragdrop_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F018-dragdrop-deep5");
    // 1) 四象限：Ctrl=Copy / Shift=Move / Ctrl+Shift=Link / 无修饰=None。
    cs.add(
        "drop_effect_modifier_matrix",
        effect_from_modifiers(MK_CONTROL) == Some(DropEffect::Copy)
            && effect_from_modifiers(MK_SHIFT) == Some(DropEffect::Move)
            && effect_from_modifiers(MK_CONTROL | MK_SHIFT) == Some(DropEffect::Link)
            && effect_from_modifiers(0).is_none(),
        "",
    );
    // 2) 无修饰交既有路由裁决（同卷 Move/跨卷 Copy——一处一事实引用）。
    cs.add(
        "no_modifier_delegates_to_route",
        effect_from_modifiers(0).is_none()
            && drop_route(true, false) == DropRoute::CopyPipeline,
        "",
    );
    // 3) 钉值锚（MK_SHIFT=0x4 / MK_CONTROL=0x8——winuser.h 原值）。
    cs.add("mk_pins", MK_SHIFT == 0x0004 && MK_CONTROL == 0x0008, "");
    cs
}
