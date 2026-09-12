//! m600gfx — VARIX-M600 AI-07 合成视觉域 (F151~F175)
//!
//! 合成器帧管线/无撕裂承诺/可变刷新协奏/HDR 全链路/色彩管理中枢/
//! 多屏色彩一致/10bit 桌面/视觉无损缩放器/子像素抗锯齿/文本渲染工坊/
//! 光标渲染直通车/遮挡剔除大师/图层合并策略/直合成通道/帧 pacing 裁判/
//! VRR 降级优雅/屏幕录制无损/远程桌面管线/虚拟显示器工坊/像素完美模式/
//! 色弱模拟器/视觉回归金样/渲染降级阶梯/显示器能力档案/合成年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F151 — 合成器帧管线：Damage → Raster → Composite → Present 顺序
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GfxStage {
    Damage,
    Raster,
    Composite,
    Present,
}

/// 阶段序号（越早越小）。
pub fn gfx_stage_order(stage: GfxStage) -> u32 {
    match stage {
        GfxStage::Damage => 0,
        GfxStage::Raster => 1,
        GfxStage::Composite => 2,
        GfxStage::Present => 3,
    }
}

/// 管线阶段必须严格按序推进（不得乱序、不得重复）。
pub fn gfx_pipeline_ordered(stages: &[GfxStage]) -> bool {
    let mut i = 1usize;
    while i < stages.len() {
        if gfx_stage_order(stages[i]) <= gfx_stage_order(stages[i - 1]) {
            return false;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// F152 — 无撕裂承诺：只在 vblank 内且画面就绪时呈现
// ===========================================================================

/// 呈现闸门：vblank 窗口内且合成结果就绪才放行。
pub fn gfx_present_allowed(in_vblank: bool, ready: bool) -> bool {
    in_vblank && ready
}

/// 撕裂判定：vblank 之外呈现即撕裂。
pub fn gfx_torn(in_vblank: bool, presented: bool) -> bool {
    presented && !in_vblank
}

// ===========================================================================
// F153 — 可变刷新协奏：帧间隔钳制进 VRR 刷新窗
// ===========================================================================

/// 想要 wanted_fps 的帧间隔（µs），但必须落在 [1e6/max_hz, 1e6/min_hz]。
/// min_hz 为 0 视为无效配置，返回 0。
pub fn gfx_vrr_interval_us(min_hz: u32, max_hz: u32, wanted_fps: u32) -> u32 {
    if min_hz == 0 || max_hz == 0 {
        return 0;
    }
    let min_interval_us = 1_000_000 / max_hz;
    let max_interval_us = 1_000_000 / min_hz;
    let wanted = if wanted_fps == 0 {
        max_interval_us
    } else {
        1_000_000 / wanted_fps
    };
    if wanted < min_interval_us {
        min_interval_us
    } else if wanted > max_interval_us {
        max_interval_us
    } else {
        wanted
    }
}

// ===========================================================================
// F154 — HDR 全链路：面板、片源具备 HDR，SDR 叠加窗必须过色调映射
// ===========================================================================

/// HDR 就绪：面板与片源都要 HDR；若有 SDR 叠加层则必须开着 tone mapper。
pub fn gfx_hdr_ready(panel_hdr: bool, source_hdr: bool, sdr_overlay: bool, tone_mapper_on: bool) -> bool {
    panel_hdr && source_hdr && (!sdr_overlay || tone_mapper_on)
}

// ===========================================================================
// F155 — 色彩管理中枢：sRGB → 线性近似（定点 v²/255，无浮点）
// ===========================================================================

/// 8bit sRGB 分量的定点线性化近似（v²，范围 0~65025）。
pub fn gfx_srgb_to_linear8(v: u8) -> u32 {
    let v = v as u32;
    v * v
}

// ===========================================================================
// F156 — 多屏色彩一致：同一色彩档案 + 伽马偏差 ≤ 10‰
// ===========================================================================

pub const GFX_GAMMA_TOLERANCE_PERMILLE: u32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GfxDisplayCfg {
    pub profile_id: u32,
    pub gamma_permille: u32,
}

/// 手写绝对差（避免 Ord）。
pub fn gfx_abs_diff_u32(a: u32, b: u32) -> u32 {
    if a >= b {
        a - b
    } else {
        b - a
    }
}

pub fn gfx_color_consistent(a: GfxDisplayCfg, b: GfxDisplayCfg) -> bool {
    a.profile_id == b.profile_id && gfx_abs_diff_u32(a.gamma_permille, b.gamma_permille) <= GFX_GAMMA_TOLERANCE_PERMILLE
}

// ===========================================================================
// F157 — 10bit 桌面：位深 → 可表达颜色数
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GfxDepth {
    D8,
    D10,
}

pub fn gfx_bpc(depth: GfxDepth) -> u32 {
    match depth {
        GfxDepth::D8 => 8,
        GfxDepth::D10 => 10,
    }
}

/// RGB 三通道可表达颜色总数（u64 防溢出）。
pub fn gfx_color_count(depth: GfxDepth) -> u64 {
    match depth {
        GfxDepth::D8 => 1u64 << 24,
        GfxDepth::D10 => 1u64 << 30,
    }
}

// ===========================================================================
// F158 — 视觉无损缩放器：只做整数倍放大
// ===========================================================================

/// 整数倍缩放系数（下取样或除 0 返回 0，即拒绝）。
pub fn gfx_integer_scale(src_px: u32, dst_px: u32) -> u32 {
    if src_px == 0 || dst_px < src_px {
        return 0;
    }
    dst_px / src_px
}

// ===========================================================================
// F159 — 子像素抗锯齿：样本覆盖率均值（permille）
// ===========================================================================

pub fn gfx_subpixel_coverage(samples: &[u32]) -> u32 {
    if samples.is_empty() {
        return 0;
    }
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < samples.len() {
        sum += samples[i];
        i += 1;
    }
    sum / samples.len() as u32
}

// ===========================================================================
// F160 — 文本渲染工坊：亚像素坐标就近整像素对齐（1/1000 px）
// ===========================================================================

/// x（1/1000 px 单位）四舍五入到整像素。
pub fn gfx_snap_x(pos_milli_px: u32) -> u32 {
    (pos_milli_px + 500) / 1000
}

// ===========================================================================
// F161 — 光标渲染直通车：小尺寸 + 无旋转才走硬件光标平面
// ===========================================================================

pub const GFX_CURSOR_MAX_PX: u32 = 64;

pub fn gfx_cursor_plane_ok(size_px: u32, rotated: bool) -> bool {
    size_px <= GFX_CURSOR_MAX_PX && !rotated
}

// ===========================================================================
// F162 — 遮挡剔除大师：覆盖 ≥ 990‰ 的区域不再绘制
// ===========================================================================

pub const GFX_OCCLUDE_PERMILLE: u32 = 990;

pub fn gfx_occluded(covered_permille: u32) -> bool {
    covered_permille >= GFX_OCCLUDE_PERMILLE
}

/// 一批区域里可剔除的数量。
pub fn gfx_cullable_count(covered: &[u32]) -> usize {
    covered.iter().filter(|&&c| gfx_occluded(c)).count()
}

// ===========================================================================
// F163 — 图层合并策略：合并后成本严格更低才合并
// ===========================================================================

pub fn gfx_merge_beneficial(cost_a: u32, cost_b: u32, cost_merged: u32) -> bool {
    cost_merged < cost_a + cost_b
}

// ===========================================================================
// F164 — 直合成通道：全屏 + 对齐 + 无变换才可直扫
// ===========================================================================

pub fn gfx_direct_scanout(fullscreen: bool, aligned: bool, transformed: bool) -> bool {
    fullscreen && aligned && !transformed
}

// ===========================================================================
// F165 — 帧 pacing 裁判：呈现时间戳单调 + 抖动在预算内
// ===========================================================================

/// 手写绝对差（u64）。
pub fn gfx_abs_diff_u64(a: u64, b: u64) -> u64 {
    if a >= b {
        a - b
    } else {
        b - a
    }
}

/// 呈现必须晚于上一次，且落在 target ± jitter 内（target = prev + interval）。
pub fn gfx_pacing_ok(prev_present_us: u64, present_us: u64, interval_us: u64, jitter_us: u64) -> bool {
    if present_us <= prev_present_us {
        return false;
    }
    let target_us = prev_present_us + interval_us;
    gfx_abs_diff_u64(present_us, target_us) <= jitter_us
}

// ===========================================================================
// F166 — VRR 降级优雅：内容帧率低于刷新下限时重复帧补足
// ===========================================================================

/// 需要的重复帧数 = ⌈min_hz / content_fps⌉；内容帧率为 0 返回 0。
pub fn gfx_vrr_fallback_repeats(content_fps: u32, min_hz: u32) -> u32 {
    if content_fps == 0 {
        return 0;
    }
    (min_hz + content_fps - 1) / content_fps
}

// ===========================================================================
// F167 — 屏幕录制无损：定容帧账本，丢帧必须记账
// ===========================================================================

pub const GFX_RECORD_CAP: u32 = 4096;

#[derive(Clone, Copy, Debug, Default)]
pub struct GfxRecorder {
    pub captured: u32,
    pub dropped: u32,
}

impl GfxRecorder {
    /// 捕获一帧。编码失败或账满都计为丢帧并返回 false。
    pub fn capture(&mut self, encode_ok: bool) -> bool {
        if !encode_ok || self.captured >= GFX_RECORD_CAP {
            self.dropped += 1;
            return false;
        }
        self.captured += 1;
        true
    }

    pub fn total(&self) -> u32 {
        self.captured + self.dropped
    }

    /// 丢帧率（‰）。空账返回 0。
    pub fn drop_permille(&self) -> u32 {
        let total = self.total();
        if total == 0 {
            0
        } else {
            self.dropped * 1000 / total
        }
    }
}

// ===========================================================================
// F168 — 远程桌面管线：capture + encode + network + decode ≤ 预算
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GfxRemotePath {
    pub capture_us: u32,
    pub encode_us: u32,
    pub network_us: u32,
    pub decode_us: u32,
}

impl GfxRemotePath {
    pub fn total_us(&self) -> u32 {
        self.capture_us + self.encode_us + self.network_us + self.decode_us
    }
}

pub fn gfx_remote_within_budget(path: GfxRemotePath, budget_us: u32) -> bool {
    path.total_us() <= budget_us
}

// ===========================================================================
// F169 — 虚拟显示器工坊：虚拟屏登记（尺寸合法 + id 去重 + 容量上限）
// ===========================================================================

pub const GFX_VIRTUAL_CAP: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct GfxVirtualDisplays {
    ids: [u32; GFX_VIRTUAL_CAP],
    len: usize,
}

impl GfxVirtualDisplays {
    pub const fn new() -> GfxVirtualDisplays {
        GfxVirtualDisplays { ids: [0; GFX_VIRTUAL_CAP], len: 0 }
    }

    /// 登记一个虚拟屏。尺寸非法、同 id 去重、工坊满（≥ 4）都拒绝。
    pub fn add(&mut self, id: u32, width_px: u32, height_px: u32) -> bool {
        if width_px == 0 || height_px == 0 || self.len >= GFX_VIRTUAL_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            if self.ids[i] == id {
                return false;
            }
            i += 1;
        }
        self.ids[self.len] = id;
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

// ===========================================================================
// F170 — 像素完美模式：整数倍缩放 + 偏移对齐到缩放栅格
// ===========================================================================

pub fn gfx_pixel_perfect(scale: u32, offset_px: u32) -> bool {
    scale >= 1 && offset_px % scale == 0
}

// ===========================================================================
// F171 — 色弱模拟器：红色通道压缩到 300‰（protanopia 近似）
// ===========================================================================

pub const GFX_PROTANOPIA_KEEP_PERMILLE: u32 = 300;

pub fn gfx_protanopia_component(component: u32) -> u32 {
    component * GFX_PROTANOPIA_KEEP_PERMILLE / 1000
}

// ===========================================================================
// F172 — 视觉回归金样：固定像素剧本 → 固定校验和
// ===========================================================================

/// 像素缓冲的折和校验和（u16 wrapping）。
pub fn gfx_golden_checksum(pixels: &[u8]) -> u16 {
    let mut sum = 0u16;
    let mut i = 0usize;
    while i < pixels.len() {
        sum = sum.wrapping_add(pixels[i] as u16);
        i += 1;
    }
    sum
}

/// 金样比对。
pub fn gfx_golden_matches(pixels: &[u8], expected: u16) -> bool {
    gfx_golden_checksum(pixels) == expected
}

// ===========================================================================
// F173 — 渲染降级阶梯：每 10 帧超预算降一级，最多降到 4 级
// ===========================================================================

pub const GFX_DEGRADE_MAX_LEVEL: u32 = 4;
pub const GFX_DEGRADE_FRAMES_PER_LEVEL: u32 = 10;

pub fn gfx_degrade_level(over_budget_frames: u32) -> u32 {
    let level = over_budget_frames / GFX_DEGRADE_FRAMES_PER_LEVEL;
    if level > GFX_DEGRADE_MAX_LEVEL {
        GFX_DEGRADE_MAX_LEVEL
    } else {
        level
    }
}

// ===========================================================================
// F174 — 显示器能力档案：刷新率 / 位深 / HDR 支持裁定
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GfxDisplayCaps {
    pub max_hz: u32,
    pub max_bpc: u32,
    pub hdr: bool,
}

impl GfxDisplayCaps {
    /// 请求的刷新率、位深、HDR 都不得超出档案能力。
    pub fn supports(&self, hz: u32, bpc: u32, hdr_needed: bool) -> bool {
        hz <= self.max_hz && bpc <= self.max_bpc && (!hdr_needed || self.hdr)
    }
}

// ===========================================================================
// F175 — 合成年报：全年 52 周覆盖 + 章节完备
// ===========================================================================

pub const GFX_ANNUAL_WEEKS: u32 = 52;
pub const GFX_ANNUAL_SECTIONS: [&str; 5] =
    ["pipeline", "color", "pacing", "record", "caps"];

pub fn gfx_annual_complete(weeks_covered: u32, sections_filled: u32) -> bool {
    weeks_covered == GFX_ANNUAL_WEEKS && sections_filled >= GFX_ANNUAL_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600gfx_checks() -> CheckSet {
    let mut set = CheckSet::new("m600gfx");

    // F151 合成器帧管线
    let good_pipe = [GfxStage::Damage, GfxStage::Raster, GfxStage::Composite, GfxStage::Present];
    let bad_pipe = [GfxStage::Raster, GfxStage::Damage];
    let dup_pipe = [GfxStage::Damage, GfxStage::Damage];
    set.add(
        "F151 pipeline order",
        gfx_pipeline_ordered(&good_pipe) && !gfx_pipeline_ordered(&bad_pipe) && !gfx_pipeline_ordered(&dup_pipe),
        "strict stage order",
    );

    // F152 无撕裂承诺
    set.add(
        "F152 tear-free gate",
        gfx_present_allowed(true, true)
            && !gfx_present_allowed(false, true)
            && !gfx_present_allowed(true, false)
            && gfx_torn(false, true)
            && !gfx_torn(true, true),
        "vblank + ready only",
    );

    // F153 可变刷新协奏
    set.add(
        "F153 vrr interval",
        gfx_vrr_interval_us(48, 144, 60) == 16666
            && gfx_vrr_interval_us(48, 144, 30) == 20833
            && gfx_vrr_interval_us(48, 144, 200) == 6944,
        "clamped into window",
    );
    set.add("F153 vrr invalid config", gfx_vrr_interval_us(0, 144, 60) == 0, "no min means invalid");

    // F154 HDR 全链路
    set.add(
        "F154 hdr chain",
        gfx_hdr_ready(true, true, false, false)
            && gfx_hdr_ready(true, true, true, true)
            && !gfx_hdr_ready(true, true, true, false)
            && !gfx_hdr_ready(false, true, false, true),
        "panel+source+tone",
    );

    // F155 色彩管理中枢
    set.add(
        "F155 srgb linear",
        gfx_srgb_to_linear8(0) == 0 && gfx_srgb_to_linear8(255) == 65025 && gfx_srgb_to_linear8(128) == 16384,
        "fixed point v^2",
    );

    // F156 多屏色彩一致
    let da = GfxDisplayCfg { profile_id: 7, gamma_permille: 2200 };
    let db = GfxDisplayCfg { profile_id: 7, gamma_permille: 2205 };
    let dc = GfxDisplayCfg { profile_id: 7, gamma_permille: 2211 };
    let dd = GfxDisplayCfg { profile_id: 8, gamma_permille: 2200 };
    set.add(
        "F156 color consistency",
        gfx_color_consistent(da, db) && !gfx_color_consistent(da, dc) && !gfx_color_consistent(da, dd),
        "same profile + gamma",
    );

    // F157 10bit 桌面
    set.add(
        "F157 depth colors",
        gfx_bpc(GfxDepth::D8) == 8 && gfx_bpc(GfxDepth::D10) == 10,
        "bpc ladder",
    );
    set.add(
        "F157 color counts",
        gfx_color_count(GfxDepth::D8) == 16_777_216 && gfx_color_count(GfxDepth::D10) == 1_073_741_824,
        "2^24 vs 2^30",
    );

    // F158 视觉无损缩放器
    set.add(
        "F158 integer scale",
        gfx_integer_scale(1080, 2160) == 2 && gfx_integer_scale(800, 2160) == 2,
        "floor scale",
    );
    set.add(
        "F158 scale rejects",
        gfx_integer_scale(2160, 1080) == 0 && gfx_integer_scale(0, 2160) == 0,
        "no downscale, no div0",
    );

    // F159 子像素抗锯齿
    let cov = [900u32, 800, 1000, 700];
    set.add("F159 subpixel coverage", gfx_subpixel_coverage(&cov) == 850 && gfx_subpixel_coverage(&[]) == 0, "mean permille");

    // F160 文本渲染工坊
    set.add(
        "F160 glyph snap",
        gfx_snap_x(499) == 0 && gfx_snap_x(500) == 1 && gfx_snap_x(1500) == 2,
        "round to pixel",
    );

    // F161 光标渲染直通车
    set.add(
        "F161 cursor fast path",
        gfx_cursor_plane_ok(32, false) && gfx_cursor_plane_ok(64, false) && !gfx_cursor_plane_ok(128, false)
            && !gfx_cursor_plane_ok(32, true),
        "small + unrotated",
    );

    // F162 遮挡剔除大师
    let regions = [995u32, 800, 990, 100];
    set.add(
        "F162 occlusion",
        gfx_occluded(990) && !gfx_occluded(989) && gfx_cullable_count(&regions) == 2,
        "990 threshold",
    );

    // F163 图层合并策略
    set.add(
        "F163 merge policy",
        gfx_merge_beneficial(100, 100, 150) && !gfx_merge_beneficial(100, 100, 250)
            && !gfx_merge_beneficial(100, 100, 200),
        "strictly cheaper",
    );

    // F164 直合成通道
    set.add(
        "F164 direct scanout",
        gfx_direct_scanout(true, true, false)
            && !gfx_direct_scanout(true, true, true)
            && !gfx_direct_scanout(true, false, false)
            && !gfx_direct_scanout(false, true, false),
        "fullscreen+aligned+plain",
    );

    // F165 帧 pacing 裁判
    set.add(
        "F165 pacing inside jitter",
        gfx_pacing_ok(0, 16700, 16666, 1000) && gfx_pacing_ok(0, 16000, 16666, 1000),
        "±jitter tolerated",
    );
    set.add(
        "F165 pacing violations",
        !gfx_pacing_ok(0, 19000, 16666, 1000) && !gfx_pacing_ok(100, 100, 16666, 1000),
        "too late or not monotonic",
    );

    // F166 VRR 降级优雅
    set.add(
        "F166 vrr fallback",
        gfx_vrr_fallback_repeats(30, 48) == 2 && gfx_vrr_fallback_repeats(60, 48) == 1
            && gfx_vrr_fallback_repeats(0, 48) == 0,
        "ceil repeats",
    );

    // F167 屏幕录制无损
    let mut rec = GfxRecorder::default();
    let mut c = 0u32;
    while c < 5 {
        rec.capture(true);
        c += 1;
    }
    let missed = rec.capture(false);
    let rec_total = rec.total();
    let rec_drop = rec.drop_permille();
    set.add(
        "F167 recorder ledger",
        !missed && rec.captured == 5 && rec_total == 6 && rec_drop == 166,
        "drop accounted",
    );

    // F168 远程桌面管线
    let path = GfxRemotePath { capture_us: 2000, encode_us: 3000, network_us: 8000, decode_us: 2000 };
    let path_total = path.total_us();
    set.add(
        "F168 remote path",
        path_total == 15000 && gfx_remote_within_budget(path, 15000) && !gfx_remote_within_budget(path, 14999),
        "sum vs budget",
    );

    // F169 虚拟显示器工坊
    let mut vd = GfxVirtualDisplays::new();
    let v1 = vd.add(1, 1920, 1080);
    let v_dup = vd.add(1, 1280, 720);
    let v_bad = vd.add(2, 0, 100);
    let v2 = vd.add(2, 1280, 720);
    set.add(
        "F169 virtual displays",
        v1 && !v_dup && !v_bad && v2 && vd.len() == 2,
        "dedup + valid size",
    );
    let mut full_vd = GfxVirtualDisplays::new();
    let mut vi = 0u32;
    while vi < GFX_VIRTUAL_CAP as u32 {
        full_vd.add(vi, 800, 600);
        vi += 1;
    }
    let v_over = full_vd.add(99, 800, 600);
    set.add("F169 virtual capped", !v_over && full_vd.len() == GFX_VIRTUAL_CAP, "cap rejects");

    // F170 像素完美模式
    set.add(
        "F170 pixel perfect",
        gfx_pixel_perfect(2, 4) && !gfx_pixel_perfect(2, 3) && !gfx_pixel_perfect(0, 0),
        "grid aligned",
    );

    // F171 色弱模拟器
    set.add(
        "F171 protanopia",
        gfx_protanopia_component(255) == 76 && gfx_protanopia_component(100) == 30
            && gfx_protanopia_component(0) == 0,
        "red kept at 30%",
    );

    // F172 视觉回归金样
    let golden_px = [1u8, 2, 3, 4];
    let drifted_px = [1u8, 2, 3, 5];
    set.add(
        "F172 golden sample",
        gfx_golden_checksum(&golden_px) == 10 && gfx_golden_matches(&golden_px, 10)
            && !gfx_golden_matches(&drifted_px, 10),
        "checksum catches drift",
    );

    // F173 渲染降级阶梯
    set.add(
        "F173 degrade ladder",
        gfx_degrade_level(5) == 0 && gfx_degrade_level(25) == 2 && gfx_degrade_level(100) == 4
            && gfx_degrade_level(999) == 4,
        "10 frames per level",
    );

    // F174 显示器能力档案
    let caps = GfxDisplayCaps { max_hz: 144, max_bpc: 10, hdr: true };
    let no_hdr_caps = GfxDisplayCaps { max_hz: 60, max_bpc: 8, hdr: false };
    set.add(
        "F174 display caps",
        caps.supports(60, 8, false)
            && caps.supports(60, 8, true)
            && !caps.supports(240, 8, false)
            && !caps.supports(60, 12, false)
            && !no_hdr_caps.supports(60, 8, true),
        "hz/bpc/hdr verdicts",
    );

    // F175 合成年报
    set.add(
        "F175 annual report",
        GFX_ANNUAL_SECTIONS.len() == 5
            && gfx_annual_complete(52, 5)
            && !gfx_annual_complete(51, 5)
            && !gfx_annual_complete(52, 4),
        "weeks + sections",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f153_vrr_interval_clamp() {
        // 下限钳制：内容 240fps 超出 144Hz 上限 → 卡在 6944µs
        assert_eq!(gfx_vrr_interval_us(48, 144, 240), 6944);
        // 上限钳制：内容 1fps → 卡在 20833µs
        assert_eq!(gfx_vrr_interval_us(48, 144, 1), 20833);
        // 无效配置
        assert_eq!(gfx_vrr_interval_us(48, 0, 60), 0);
    }

    #[test]
    fn f165_pacing_boundaries() {
        let interval = 16666u64;
        assert!(gfx_pacing_ok(0, 16666, interval, 0)); // 恰好命中
        assert!(gfx_pacing_ok(0, 17666, interval, 1000)); // 抖动上界
        assert!(!gfx_pacing_ok(0, 17667, interval, 1000)); // 超一个 µs
        assert!(!gfx_pacing_ok(5000, 4000, interval, 1000)); // 回退
    }

    #[test]
    fn f167_recorder_never_overfills() {
        let mut rec = GfxRecorder::default();
        let mut i = 0u32;
        while i < GFX_RECORD_CAP {
            assert!(rec.capture(true));
            i += 1;
        }
        assert!(!rec.capture(true)); // 账满拒绝并计丢帧
        assert_eq!(rec.captured, GFX_RECORD_CAP);
        assert_eq!(rec.dropped, 1);
        assert_eq!(rec.drop_permille(), 0); // 1/4097 → 0‰
    }

    #[test]
    fn f169_virtual_display_dedup_cap() {
        let mut vd = GfxVirtualDisplays::new();
        assert!(vd.add(9, 3840, 2160));
        assert!(!vd.add(9, 1920, 1080)); // id 去重
        assert!(!vd.add(10, 1920, 0)); // 尺寸非法
        let mut i = 0u32;
        while vd.len() < GFX_VIRTUAL_CAP {
            assert!(vd.add(100 + i, 800, 600));
            i += 1;
        }
        assert!(!vd.add(999, 800, 600)); // 容量上限
        assert_eq!(vd.len(), GFX_VIRTUAL_CAP);
    }

    #[test]
    fn f172_golden_checksum_stable() {
        let px = [7u8; 100];
        assert_eq!(gfx_golden_checksum(&px), 700);
        // 环绕语义：65025 + 100 → (u16) 折回
        let mut big = [0u8; 4];
        big[0] = 255;
        big[1] = 255;
        assert_eq!(gfx_golden_checksum(&big), 510);
    }

    #[test]
    fn f175_gfx_selfcheck_all_pass() {
        let set = run_m600gfx_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
