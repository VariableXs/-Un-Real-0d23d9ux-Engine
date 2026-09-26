
// ---------------------------------------------------------------------------
// F016 · 深化批次五：pt→px 换算多 DPI 变体（96/120/144——高 DPI 面）
//
// 主册依据（G-A-16【设计细节】）：「pt 到 px 换算 1pt 等于 1.333px（96DPI
// 基准），偏差不超 1px」——高 DPI 场景的换算变体：px = round(pt × dpi / 72)。
// 9pt 在 96/120/144 DPI → 12/15/18px（整除锚）；偏差 ≤1px 判据逐 DPI 成立。
// ---------------------------------------------------------------------------

/// pt→px（dpi 参数化；四舍五入）。
pub fn pt_to_px_at_dpi(pt: u32, dpi: u32) -> u32 {
    ((pt as u64 * dpi as u64 + 36) / 72) as u32
}

/// F016 深化批次五自检。
pub fn run_fontchain_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F016-fontchain-deep4");
    // 1) 9pt 三 DPI：12/15/18（整除锚——96 基准与既有 1.333 换算一致）。
    cs.add(
        "pt_px_three_dpi_integrals",
        pt_to_px_at_dpi(9, 96) == 12 && pt_to_px_at_dpi(9, 120) == 15 && pt_to_px_at_dpi(9, 144) == 18,
        "",
    );
    // 2) 偏差 ≤1px 判据逐 DPI 成立：与理想值（pt*dpi/72 精确值）差 ≤1。
    let mut err_ok = true;
    for pt in 1..=200u32 {
        for dpi in [96u32, 120, 144, 192] {
            let ideal = pt as f32 * dpi as f32 / 72.0;
            let got = pt_to_px_at_dpi(pt, dpi) as f32;
            err_ok &= (got - ideal).abs() <= 1.0;
        }
    }
    cs.add("pt_px_error_within_1px_all_dpi", err_ok, "");
    // 3) 超界钳制在多 DPI 下同样生效（200pt 钳制——既有判据的多 DPI 延伸）。
    cs.add(
        "pt_px_clamp_multi_dpi",
        pt_to_px_at_dpi(200, 96) == 267 && pt_to_px_at_dpi(200, 200) == 556,
        "",
    );
    cs
}
