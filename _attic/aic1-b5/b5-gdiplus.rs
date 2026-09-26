
// ---------------------------------------------------------------------------
// F007 · 深化批次五：灰度 AA 覆盖率混合核（抗锯齿的最后一步——真混合）
//
// 主册依据（G-A-07【设计细节】）：「抗锯齿统一走 VARIX 文本管线灰度 AA」
// ——灰度 AA 的落点是覆盖率混合：边像素 = src·cov + dst·(255−cov)，逐通道
// 四舍五入可复现（对拍判据的确定性根基）。
// ---------------------------------------------------------------------------

/// 覆盖率混合（cov 0..255：0 = 全目标色，255 = 全源色；逐通道 (a*cov + b*
/// (255-cov) + 127) / 255 四舍五入——确定性混合，对拍可复现）。
pub fn blend_coverage(src: u32, dst: u32, cov: u8) -> u32 {
    let mut out = 0u32;
    for shift in [0u32, 8, 16, 24] {
        let s = ((src >> shift) & 0xFF) as u32;
        let d = ((dst >> shift) & 0xFF) as u32;
        let v = (s * cov as u32 + d * (255 - cov as u32) + 127) / 255;
        out |= (v & 0xFF) << shift;
    }
    out
}

/// F007 深化批次五自检。
pub fn run_gdiplus_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F007-gdiplus-deep4");
    // 1) 端点：cov=0 → 全目标；cov=255 → 全源；cov=128 → 中点（±1 舍入）。
    let mid = blend_coverage(0xFFFF_FFFF, 0xFF00_0000, 128);
    let mid_b = (mid & 0xFF) as i32;
    let mid_r = ((mid >> 16) & 0xFF) as i32;
    cs.add(
        "aa_blend_endpoints",
        blend_coverage(0xFFFF_FFFF, 0xFF00_0000, 0) == 0xFF00_0000
            && blend_coverage(0xFFFF_FFFF, 0xFF00_0000, 255) == 0xFFFF_FFFF
            && (mid_b - 127).abs() <= 1
            && mid_r == 255,
        "",
    );
    // 2) 确定性：同输入两次混合结果逐位一致（对拍可复现的根基）。
    let a1 = blend_coverage(0x12_34_56_78, 0x9A_BC_DE_F0, 77);
    let a2 = blend_coverage(0x12_34_56_78, 0x9A_BC_DE_F0, 77);
    cs.add("aa_blend_deterministic", a1 == a2, "");
    // 3) 单调性：cov 递增 → 通道值单调不减（从目标色向源色过渡无回跳）。
    let mut mono = true;
    let mut prev = 0u32;
    for cov in [0u8, 32, 64, 96, 128, 160, 192, 224, 255] {
        let v = blend_coverage(0xFF00_00FF, 0xFF00_0000, cov) & 0xFF;
        mono &= v >= prev;
        prev = v;
    }
    cs.add("aa_blend_monotonic", mono, "");
    cs
}
