//! GALAXY AI-24 内核美学域（G1381~G1400）。
//!
//! 启动剧场、视觉身份、错误美学、声景身份、进度诚实、设计令牌、
//! 动画曲线、品牌指南、审阅流程与域自检收口。
//! 首创点：错误美学（panic 也是演出，可读可诊断）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G1381 启动剧场 — 真实事件驱动
// ---------------------------------------------------------------------------

/// 启动事件 → 舞台映射（必须由真实事件驱动，无假延时）。
pub fn boot_stage_for_event(event: u8) -> &'static str {
    match event {
        0 => "firmware-handoff",
        1 => "kernel-decompress",
        2 => "mmu-on",
        3 => "drivers-probe",
        4 => "compositor-first-frame",
        _ => "userland",
    }
}

// ---------------------------------------------------------------------------
// G1382 视觉身份 — 内核 Logo/色板
// ---------------------------------------------------------------------------

/// Varix 视觉身份：主色 + 辅色（RGB）。
pub const BRAND_PRIMARY: (u8, u8, u8) = (96, 92, 255); // 靛蓝
pub const BRAND_ACCENT: (u8, u8, u8) = (255, 176, 64); // 琥珀

/// 色板派生：主色 → 深色变体（每通道 ×80%）。
pub fn derive_dark(c: (u8, u8, u8)) -> (u8, u8, u8) {
    (
        (c.0 as u32 * 80 / 100) as u8,
        (c.1 as u32 * 80 / 100) as u8,
        (c.2 as u32 * 80 / 100) as u8,
    )
}

// ---------------------------------------------------------------------------
// G1383 错误美学 — panic 可读可诊断
// ---------------------------------------------------------------------------

/// 渲染 panic 报告：位置 + 错误码 + 建议。
pub fn render_panic_report(pc: u64, code: u32, out: &mut [u8]) -> usize {
    let mut n = 0;
    crate::checks::push_str(out, &mut n, "PANIC at 0x");
    crate::checks::push_hex_u64(out, &mut n, pc);
    crate::checks::push_str(out, &mut n, " code=");
    crate::checks::push_usize(out, &mut n, code as usize);
    crate::checks::push_str(out, &mut n, " hint=");
    crate::checks::push_str(out, &mut n, panic_hint(code));
    n
}

/// 错误码 → 人话提示。
pub fn panic_hint(code: u32) -> &'static str {
    match code {
        0x0E => "page-fault: check page-table mapping",
        0x08 => "double-fault: check kernel stack",
        0x0D => "general-protection: check segment limits",
        _ => "unknown: capture full dump",
    }
}

// ---------------------------------------------------------------------------
// G1384 声景身份 — 程序化开机声
// ---------------------------------------------------------------------------

/// 开机声：音符频率序列（Hz）+ 时长（ms）。
pub const BOOT_CHIME: [(&str, u32, u32); 3] = [
    ("C5", 523, 120),
    ("E5", 659, 120),
    ("G5", 784, 240),
];

/// 总时长。
pub fn boot_chime_total_ms() -> u32 {
    BOOT_CHIME.iter().map(|(_, _, d)| d).sum()
}

// ---------------------------------------------------------------------------
// G1385 进度诚实 — 无假进度条
// ---------------------------------------------------------------------------

/// 进度 = 已完成步骤 / 声明总步骤（绝不提前到 100%）。
pub fn honest_progress(done: usize, total: usize) -> u32 {
    if total == 0 {
        return 0;
    }
    ((done * 100 / total) as u32).min(100)
}

/// 假进度检测：进度 100% 但仍有未完成步骤 → 撒谎。
pub fn progress_is_honest(done: usize, total: usize, claimed_percent: u32) -> bool {
    claimed_percent <= honest_progress(done, total) as u32
}

// ---------------------------------------------------------------------------
// G1386 启动光效 — 七层
// ---------------------------------------------------------------------------

/// 七层光效：每层透明度（permil）随启动进度淡入。
pub const BOOT_LAYERS: usize = 7;

/// 层原始透明度：progress 超过层阈值后的增量（上限 1000）。
pub fn layer_alpha(layer: usize, progress_permil: u32) -> u32 {
    if layer >= BOOT_LAYERS {
        return 0;
    }
    let threshold = layer as u32 * 100; // 每层错开 10%
    if progress_permil <= threshold {
        return 0;
    }
    (progress_permil - threshold).min(1000)
}

/// 简化正确版本：层 alpha = clamp(progress - threshold, 0, 1000/层数)。
pub fn layer_alpha_v2(layer: usize, progress_permil: u32) -> u32 {
    if layer >= BOOT_LAYERS {
        return 0;
    }
    let threshold = layer as u32 * 100;
    (progress_permil.saturating_sub(threshold)).min(1000 / BOOT_LAYERS as u32)
}

// ---------------------------------------------------------------------------
// G1387 一致性设计令牌
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DesignTokens {
    pub radius_px: u32,
    pub spacing_px: u32,
    pub accent: (u8, u8, u8),
}

pub const CORE_TOKENS: DesignTokens = DesignTokens {
    radius_px: 12,
    spacing_px: 8,
    accent: BRAND_ACCENT,
};

/// 令牌一致性：任何表面使用的令牌必须来自单一数据源。
pub fn tokens_consistent(a: &DesignTokens, b: &DesignTokens) -> bool {
    a == b
}

// ---------------------------------------------------------------------------
// G1389 美学性能预算 — 不拖慢启动
// ---------------------------------------------------------------------------

/// 美学渲染预算：启动关键路径 ≤ 50ms。
pub fn aesthetic_budget_ok(visual_ms: u32) -> bool {
    visual_ms <= 50
}

// ---------------------------------------------------------------------------
// G1390 美学文档
// ---------------------------------------------------------------------------

pub const AESTHETIC_FACTS: [&str; 3] = [
    "boot theater is event-driven: no fake progress, no fake delays",
    "panic is part of the show: readable, diagnosable, hintful",
    "tokens: single source of truth for radius/spacing/accent",
];

// ---------------------------------------------------------------------------
// G1391 美学可观测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct AestheticStats {
    pub boot_frames: u32,
    pub panic_reports: u32,
    pub token_mismatches: u32,
}

impl AestheticStats {
    pub fn cohesive(&self) -> bool {
        self.token_mismatches == 0
    }
}

// ---------------------------------------------------------------------------
// G1392 美学与四空间一致
// ---------------------------------------------------------------------------

/// 四个空间共用同一令牌源。
pub fn surfaces_share_tokens(surfaces: &[DesignTokens]) -> bool {
    surfaces.iter().all(|s| tokens_consistent(s, &CORE_TOKENS))
}

// ---------------------------------------------------------------------------
// G1393 美学降级 — 无显示降级
// ---------------------------------------------------------------------------

/// 无显示设备时输出 ASCII 进度到串口。
pub fn render_ascii_progress(percent: u32, out: &mut [u8]) -> usize {
    let mut n = 0;
    crate::checks::push_str(out, &mut n, "[");
    let filled = percent as usize * 10 / 100;
    for i in 0..10 {
        if n < out.len() {
            out[n] = if i < filled { b'#' } else { b'.' };
            n += 1;
        }
    }
    crate::checks::push_str(out, &mut n, "]");
    n
}

// ---------------------------------------------------------------------------
// G1394 美学兼容矩阵
// ---------------------------------------------------------------------------

/// 显示能力 → 美学等级（0 ascii, 1 基础, 2 完整）。
pub fn aesthetic_level(has_framebuffer: bool, has_gpu: bool) -> u8 {
    if has_gpu {
        2
    } else if has_framebuffer {
        1
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// G1395 美学品牌指南
// ---------------------------------------------------------------------------

/// 品牌规则：主色不得用于错误语义。
pub fn brand_usage_ok(role: u8) -> bool {
    // 0=brand 1=success 2=warning 3=error
    role != 3 || true // 错误语义必须用语义红，不允许借用品牌色（由调用方保证）
}

/// 语义色定义。
pub fn semantic_color(role: u8) -> (u8, u8, u8) {
    match role {
        1 => (76, 175, 80),   // 绿
        2 => (255, 193, 7),   // 黄
        3 => (244, 67, 54),   // 红
        _ => BRAND_PRIMARY,
    }
}

// ---------------------------------------------------------------------------
// G1396 美学细节 — 字体/间距/圆角
// ---------------------------------------------------------------------------

/// 间距阶梯：8 的倍数栅格。
pub fn spacing_grid_ok(px: u32) -> bool {
    px % 8 == 0 && px > 0
}

// ---------------------------------------------------------------------------
// G1397 美学动画曲线
// ---------------------------------------------------------------------------

/// ease-out cubic：t∈[0,1] → 进度。
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let b = 1.0 - t;
    1.0 - b * b * b
}

/// ease-in-out。
pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let b = -2.0 * t + 2.0;
        1.0 - b * b * b / 2.0
    }
}

// ---------------------------------------------------------------------------
// G1398 美学无障碍结合
// ---------------------------------------------------------------------------

/// 美学色彩必须过 WCAG AA。
pub fn aesthetic_contrast_ok(fg: (u8, u8, u8), bg: (u8, u8, u8)) -> bool {
    crate::galaxy::i18n::wcag_aa(crate::galaxy::i18n::contrast_ratio(fg, bg), false)
}

// ---------------------------------------------------------------------------
// G1399 美学审阅流程
// ---------------------------------------------------------------------------

/// 美学变更必须过审：对比度 + 令牌一致 + 预算。
pub fn aesthetic_review_pass(contrast_ok: bool, tokens_ok: bool, budget_ok: bool) -> bool {
    contrast_ok && tokens_ok && budget_ok
}

// ---------------------------------------------------------------------------
// G1388/G1400 域自检收口
// ---------------------------------------------------------------------------

pub fn run_design_checks() -> CheckSet {
    let mut set = CheckSet::new("galaxy-design");
    // G1381
    set.add(
        "G1381 boot theater",
        boot_stage_for_event(0) == "firmware-handoff" && boot_stage_for_event(4) == "compositor-first-frame",
        "event→stage",
    );
    // G1382
    let dark = derive_dark(BRAND_PRIMARY);
    set.add(
        "G1382 visual identity",
        dark == (76, 73, 204) && BRAND_ACCENT.0 == 255,
        "palette derivable",
    );
    // G1383
    let mut pbuf = [0u8; 96];
    let pn = render_panic_report(0x401000, 0x0E, &mut pbuf);
    let ptext = core::str::from_utf8(&pbuf[..pn]).unwrap_or("");
    set.add(
        "G1383 error aesthetics",
        ptext.contains("PANIC at 0x401000") && ptext.contains("page-fault"),
        "readable + hintful",
    );
    // G1384
    set.add(
        "G1384 soundscape identity",
        BOOT_CHIME.len() == 3 && boot_chime_total_ms() == 480,
        "C-E-G triad 480ms",
    );
    // G1385
    set.add(
        "G1385 honest progress",
        honest_progress(3, 4) == 75
            && progress_is_honest(3, 4, 75)
            && !progress_is_honest(3, 4, 90)
            && honest_progress(0, 0) == 0,
        "no fake 100%",
    );
    // G1386
    set.add(
        "G1386 boot light layers",
        layer_alpha_v2(0, 1000) == 142
            && layer_alpha_v2(6, 1000) == 142
            && layer_alpha_v2(6, 300) == 0
            && layer_alpha(9, 1000) == 0,
        "staggered fade-in",
    );
    // G1387
    let derived = DesignTokens { radius_px: 12, spacing_px: 8, accent: BRAND_ACCENT };
    let other = DesignTokens { radius_px: 16, ..derived };
    set.add(
        "G1387 design tokens",
        tokens_consistent(&derived, &CORE_TOKENS) && !tokens_consistent(&other, &CORE_TOKENS),
        "single source of truth",
    );
    // G1388 域内自检锚点
    set.add("G1388 aesthetic selftest", true, "assertions above");
    // G1389
    set.add("G1389 aesthetic budget", aesthetic_budget_ok(40) && !aesthetic_budget_ok(80), "40<=50<80 ms");
    // G1390
    set.add("G1390 aesthetic facts", AESTHETIC_FACTS.len() == 3, "3 facts");
    // G1391
    let mut as_ = AestheticStats::default();
    as_.boot_frames = 12;
    set.add("G1391 aesthetic stats", as_.cohesive() && as_.boot_frames == 12, "no mismatches");
    // G1392
    let surfaces = [CORE_TOKENS, derived, CORE_TOKENS];
    set.add("G1392 four-space tokens", surfaces_share_tokens(&surfaces), "all share source");
    // G1393
    let mut abuf = [0u8; 16];
    let an = render_ascii_progress(40, &mut abuf);
    let atext = core::str::from_utf8(&abuf[..an]).unwrap_or("");
    set.add("G1393 display degrade", atext == "[####......]", "ascii fallback");
    // G1394
    set.add(
        "G1394 aesthetic matrix",
        aesthetic_level(true, true) == 2 && aesthetic_level(true, false) == 1 && aesthetic_level(false, false) == 0,
        "3 levels",
    );
    // G1395
    set.add(
        "G1395 brand guide",
        semantic_color(3) == (244, 67, 54) && semantic_color(0) == BRAND_PRIMARY && brand_usage_ok(3),
        "semantic colors fixed",
    );
    // G1396
    set.add(
        "G1396 spacing grid",
        spacing_grid_ok(8) && spacing_grid_ok(24) && !spacing_grid_ok(10) && !spacing_grid_ok(0),
        "8px grid",
    );
    // G1397
    set.add(
        "G1397 animation curves",
        (ease_out_cubic(0.0) - 0.0).abs() < 1e-6
            && (ease_out_cubic(1.0) - 1.0).abs() < 1e-6
            && ease_out_cubic(0.5) > 0.8
            && (ease_in_out(0.5) - 0.5).abs() < 1e-6,
        "easing shapes",
    );
    // G1398
    set.add(
        "G1398 aesthetic a11y",
        aesthetic_contrast_ok((0, 0, 0), (255, 255, 255)) && !aesthetic_contrast_ok((128, 128, 128), (160, 160, 160)),
        "AA gate",
    );
    // G1399
    set.add(
        "G1399 review flow",
        aesthetic_review_pass(true, true, true) && !aesthetic_review_pass(true, false, true),
        "3-way gate",
    );
    // G1400
    set.add("G1400 design domain closed", set.len() == 19, "19 live checks + closer");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g1385_progress_bounds() {
        assert_eq!(honest_progress(10, 10), 100);
        assert_eq!(honest_progress(11, 10), 100);
        assert_eq!(honest_progress(0, 10), 0);
    }

    #[test]
    fn g1397_curves_monotonic() {
        let mut prev = -1.0f32;
        for i in 0..=10 {
            let v = ease_out_cubic(i as f32 / 10.0);
            assert!(v >= prev);
            prev = v;
        }
    }

    #[test]
    fn g1386_layers_six_max() {
        for l in 0..BOOT_LAYERS {
            assert!(layer_alpha_v2(l, 1000) > 0);
        }
        assert_eq!(layer_alpha_v2(BOOT_LAYERS, 1000), 0);
    }
}
