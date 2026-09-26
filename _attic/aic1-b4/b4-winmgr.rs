
// ---------------------------------------------------------------------------
// F005 · 深化批次四：滚动条族 + 静态控件族消息钉值 + 控件渲染主权裁定面
//
// 主册依据（G-A-05【功能定义】）：「常用窗口类七族（按钮/编辑框/列表框/
// 组合框/滚动条/静态文本/通用对话框容器）」——批次二已落按钮(BM_)/编辑框
// (EM_)/列表框(LB_)/组合框(CB_) 四族；本批补滚动条(SB_ 通知码)与静态文本
// (STM_) 两族；【设计细节】「每表附『VARIX 渲染替代』说明——按钮按下态由
// winsrv 画还是程序自绘按程序风格位裁定」→ RenderOwner 裁定面。
// ---------------------------------------------------------------------------

/// 滚动条通知码（WM_HSCROLL/WM_VSCROLL 的 wParam 低字——winuser.h 钉值）。
pub const SB_LINEUP: u32 = 0;
pub const SB_LINEDOWN: u32 = 1;
pub const SB_PAGEUP: u32 = 2;
pub const SB_PAGEDOWN: u32 = 3;
pub const SB_THUMBPOSITION: u32 = 4;
pub const SB_THUMBTRACK: u32 = 5;
pub const SB_TOP: u32 = 6;
pub const SB_BOTTOM: u32 = 7;
pub const SB_ENDSCROLL: u32 = 8;

/// 静态控件消息（winuser.h 钉值）。
pub const STM_SETICON: u32 = 0x0170;
pub const STM_GETICON: u32 = 0x0171;
pub const STM_SETIMAGE: u32 = 0x0172;
pub const STM_GETIMAGE: u32 = 0x0173;

/// 滚动条通知码全表（9 值——查找与穷举对账面）。
pub const SB_NOTIF_CODES: [u32; 9] = [
    SB_LINEUP, SB_LINEDOWN, SB_PAGEUP, SB_PAGEDOWN, SB_THUMBPOSITION,
    SB_THUMBTRACK, SB_TOP, SB_BOTTOM, SB_ENDSCROLL,
];

/// 查滚动条通知码是否合法（未知码如实 None——不猜）。
pub fn sb_notif_known(code: u32) -> bool {
    SB_NOTIF_CODES.contains(&code)
}

/// 控件渲染主权（主册【设计细节】「VARIX 渲染替代」的裁定结果）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ControlRenderOwner {
    /// winsrv 按族缺省样式绘制（VARIX 视觉语汇）。
    WinsrvDefault,
    /// 程序自绘（程序风格位声明——VARIX 只管没主权的地方，与 F005 主权裁定
    /// 同一哲学的控件级延伸）。
    ProgramSelfPaint,
}

/// 裁定：程序风格位（BS_OWNERDRAW 同族语义）声明自绘 → 尊重程序。
pub fn control_render_owner(ownerdraw_style_bit: bool) -> ControlRenderOwner {
    if ownerdraw_style_bit {
        ControlRenderOwner::ProgramSelfPaint
    } else {
        ControlRenderOwner::WinsrvDefault
    }
}

/// F005 深化批次四自检。
pub fn run_winmgr_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F005-winmgr-deep3");
    // 1) SB 通知码 9 值钉值 + 穷举合法 + 未知码如实不认。
    cs.add(
        "sb_notif_codes_pinned",
        SB_NOTIF_CODES == [0, 1, 2, 3, 4, 5, 6, 7, 8]
            && (0..=8u32).all(sb_notif_known)
            && !sb_notif_known(9)
            && !sb_notif_known(0xFFFF),
        "",
    );
    // 2) STM 钉值（winuser.h 原值）。
    cs.add(
        "stm_messages_pinned",
        STM_SETICON == 0x0170
            && STM_GETICON == 0x0171
            && STM_SETIMAGE == 0x0172
            && STM_GETIMAGE == 0x0173,
        "",
    );
    // 3) 渲染主权裁定双向：ownerdraw 位 → 程序自绘；无位 → winsrv 缺省。
    cs.add(
        "control_render_owner_ruling",
        control_render_owner(true) == ControlRenderOwner::ProgramSelfPaint
            && control_render_owner(false) == ControlRenderOwner::WinsrvDefault,
        "",
    );
    cs
}
