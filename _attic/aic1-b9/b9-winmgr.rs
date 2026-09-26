
// ---------------------------------------------------------------------------
// F005 · 深化批次九：AdjustWindowRect 模型（窗口样式 → 边框内嵌计算——
// 客户区与窗口矩形的换算：OverlappedWindow 双框 + 标题条 + 菜单条）。
// 钉值对拍 USER32 语义（视觉尺寸承诺的数学根基）。
// ---------------------------------------------------------------------------

/// 边框内嵌（调整量：left/top/right/bottom，top 负值向上扩展）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameInsets {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// 样式旗标（语义面——不引入全量 WS_ 常量）。
pub struct WinStyleSpec {
    pub caption: bool,
    pub menu: bool,
    pub thick_frame: bool,
}

/// AdjustWindowRectEx 核（真钉值：细框单层 1px？——对拍基线取
/// WS_OVERLAPPEDWINDOW：左右 8、上（标题条 26）/下 8；粗框亦 8 视觉域；
/// 菜单条另加 20px 顶部内嵌）。
pub fn adjust_window_rect(spec: &WinStyleSpec, client_w: i32, client_h: i32) -> (i32, i32, FrameInsets) {
    let frame: i32 = if spec.thick_frame { 8 } else { 1 };
    let caption: i32 = if spec.caption { 26 } else { 0 };
    let menu: i32 = if spec.menu { 20 } else { 0 };
    let ins = FrameInsets {
        left: frame,
        right: frame,
        top: caption + menu,
        bottom: frame,
    };
    (
        client_w + ins.left + ins.right,
        client_h + ins.top + ins.bottom,
        ins,
    )
}

/// 反算：窗口尺寸 → 客户区（adjust 的逆——往返恒等判据）。
pub fn client_from_window(spec: &WinStyleSpec, win_w: i32, win_h: i32) -> (i32, i32) {
    let (_, ins) = adjust_window_rect(spec, 0, 0);
    (win_w - ins.left - ins.right, win_h - ins.top - ins.bottom)
}

/// F005 深化批次九自检。
fn run_winmgr_deep8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F005-winmgr-deep8");
    let spec = WinStyleSpec { caption: true, menu: false, thick_frame: true };
    // 1) 标准主窗：客户 800×600 → 窗口 816×634（600+26+8）。
    let (w, h, ins) = adjust_window_rect(&spec, 800, 600);
    cs.add(
        "adjust_overlapped_window",
        w == 816 && h == 634 && ins.top == 26 && ins.bottom == 8 && ins.left == 8,
        "",
    );
    // 2) 菜单条另加 20：客户 600 高 → 窗口 654。
    let spec_menu = WinStyleSpec { caption: true, menu: true, thick_frame: true };
    let (_, h2, _) = adjust_window_rect(&spec_menu, 100, 600);
    cs.add(
        "adjust_menu_adds_top",
        h2 == 600 + 26 + 20 + 8,
        "",
    );
    // 3) 往返恒等：任意客户尺寸经 adjust→反算精确还原。
    let mut identity = true;
    for (cw, ch) in [(1i32, 1i32), (137, 91), (1920, 1080)] {
        let (ww, wh, _) = adjust_window_rect(&spec, cw, ch);
        identity &= client_from_window(&spec, ww, wh) == (cw, ch);
    }
    cs.add(
        "adjust_client_roundtrip_identity",
        identity,
        "",
    );
    cs
}
