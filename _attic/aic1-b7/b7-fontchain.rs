
// ---------------------------------------------------------------------------
// F016 · 深化批次七：字号阶梯吸附（设置页字号选择的一致性面）
//
// 主册依据（G-A-16【验收判据】）+【规格框架】——常用字号阶梯（9/10/11/12/14/
// 16/18/22——Windows 字号对话框同阶梯）；任意 pt 输入吸附最近阶梯（平手取小，
// 防膨胀——确定性）。
// ---------------------------------------------------------------------------

/// 常用字号阶梯（8 档——字体对话框/设置页同源）。
pub const FONT_SIZE_LADDER: [u32; 8] = [9, 10, 11, 12, 14, 16, 18, 22];

/// 最近阶梯吸附（平手取小——确定性规则一处一事实）。
pub fn snap_font_size(pt: u32) -> u32 {
    let mut best = FONT_SIZE_LADDER[0];
    let mut best_dist = pt.saturating_sub(FONT_SIZE_LADDER[0]) | (FONT_SIZE_LADDER[0].saturating_sub(pt));
    // 统一距离：|pt - step|。
    best_dist = if pt > best { pt - best } else { best - pt };
    for &step in FONT_SIZE_LADDER.iter() {
        let d = if pt > step { pt - step } else { step - pt };
        if d < best_dist {
            best_dist = d;
            best = step;
        }
    }
    best
}

/// F016 深化批次七自检。
pub fn run_fontchain_deep6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F016-fontchain-deep6");
    // 1) 阶梯值直通：9/12/22 原样返回。
    cs.add(
        "font_size_ladder_direct",
        snap_font_size(9) == 9 && snap_font_size(12) == 12 && snap_font_size(22) == 22,
        "",
    );
    // 2) 吸附：13 平手取小（12）；15 平手取小（14）；17 → 16；20 → 22？20-18=2,
    //    22-20=2 平手 → 取小 18。
    cs.add(
        "font_size_snap_tie_to_lower",
        snap_font_size(13) == 12 && snap_font_size(15) == 14 && snap_font_size(17) == 16
            && snap_font_size(20) == 18,
        "",
    );
    // 3) 越界钳制：1pt → 9（下界）；100pt → 22（上界）。
    cs.add(
        "font_size_clamp_bounds",
        snap_font_size(1) == 9 && snap_font_size(100) == 22,
        "",
    );
    cs
}
