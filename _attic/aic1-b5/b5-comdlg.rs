
// ---------------------------------------------------------------------------
// F008 · 深化批次五：对话框 DPI 缩放核（多分辨率成立性的换算面）
//
// 主册依据（G-A-08【交互设计】）：「文件对话框按 C-4 资源管理器组件复用（树
// 窗格 200px/…）」+【状态与异常】多 DPI——基线 100% 尺寸在 125%/150%/200%
// 下的换算（四舍五入 ±1px 容差——乙节基线 ±2px 纪律内）。
// ---------------------------------------------------------------------------

/// DPI 缩放（permille 缩放系数：100% = 1000；四舍五入换算）。
pub fn dpi_scale_px(base_px: u32, scale_permille: u32) -> u32 {
    (base_px * scale_permille as u64 + 500) as u32 / 1000
}

/// 四档标准缩放（4K 走查的四档——通十二查第 4 条同源）。
pub const DPI_SCALES_PERMILLE: [u32; 4] = [1000, 1250, 1500, 2000];

/// F008 深化批次五自检。
pub fn run_comdlg_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F008-comdlg-deep4");
    // 1) 树窗格 200px 四档：200/250/300/400（整除精确——布局不破版）。
    let four = |s: u32| dpi_scale_px(200, s);
    cs.add(
        "dialog_dpi_tree_pane_four_scales",
        four(1000) == 200 && four(1250) == 250 && four(1500) == 300 && four(2000) == 400,
        "",
    );
    // 2) 非整除换算四舍五入：33px @125% = 41.25 → 41（±1 容差内）。
    cs.add(
        "dialog_dpi_rounding",
        dpi_scale_px(33, 1250) == 41 && dpi_scale_px(9, 1500) == 14, // 13.5 → 14（四舍五入）
        "",
    );
    // 3) 缩放表钉值锚（四档与通十二查第 4 条同源）。
    cs.add(
        "dialog_dpi_scales_pinned",
        DPI_SCALES_PERMILLE == [1000, 1250, 1500, 2000],
        "",
    );
    cs
}
