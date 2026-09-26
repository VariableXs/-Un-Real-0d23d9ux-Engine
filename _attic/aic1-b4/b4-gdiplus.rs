
// ---------------------------------------------------------------------------
// F007 · 深化批次四：对拍容差分区路由（非文本逐像素 / 文本 SSIM）
//
// 主册依据（G-A-07【设计细节】）：「对拍容差规则文档化：非文本区逐像素容差
// 1/255、文本区按 SSIM 大于 0.95——容差也是判据不是感觉」。既有面：像素容差
// 与 SSIM 计算核不重复；本段补**分区路由**（一块对拍区域按性质走对应判据，
/// 不许拿文本判据放行非文本区，也不许拿像素容差否决合法字体 hinting 差异）。
// ---------------------------------------------------------------------------

/// 对拍区域性质（决定走哪条容差判据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompareRegion {
    /// 文本区：SSIM ≥ 0.95（hinting 差异合法——逐像素判据不适用）。
    Text,
    /// 非文本区：逐像素容差 ≤ 1/255。
    NonText,
}

/// 分区容差裁决：`pixel_delta` = 区域最大像素差；`ssim_permille` = 区域 SSIM
/// （permille）。文本区只看 SSIM；非文本区只看像素差——**互不串用**。
pub fn tolerance_ok(region: CompareRegion, pixel_delta: u8, ssim_permille: u32) -> bool {
    match region {
        CompareRegion::Text => ssim_permille >= TEXT_SSIM_PERMILLE,
        CompareRegion::NonText => pixel_delta <= PIXEL_TOLERANCE,
    }
}

/// F007 深化批次四自检。
pub fn run_gdiplus_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F007-gdiplus-deep3");
    // 1) 非文本区：像素差 1（恰容差内）过；2 出界——SSIM 值不参与（互不串用）。
    cs.add(
        "nontext_pixel_only",
        tolerance_ok(CompareRegion::NonText, 1, 0)
            && !tolerance_ok(CompareRegion::NonText, 2, 999),
        "",
    );
    // 2) 文本区：SSIM 950（恰达线）过；949 出界——像素差大（hinting）不否决。
    cs.add(
        "text_ssim_only",
        tolerance_ok(CompareRegion::Text, 255, 950)
            && !tolerance_ok(CompareRegion::Text, 0, 949),
        "",
    );
    // 3) 判据线钉值锚（与既有常量同源——一处一事实）。
    cs.add(
        "tolerance_consts_anchor",
        PIXEL_TOLERANCE == 1 && TEXT_SSIM_PERMILLE == 950,
        "",
    );
    cs
}
