//! m600media — VARIX-M600 AI-11 图像与媒体域 (F251~F275)
//!
//! 图像解码矩阵/GPU 编解码直通/格式先锋队/截图工坊/录屏导演台/直播管线/
//! 相册时间轴/智能相册整理/人脸聚簇/图像搜索/EXIF 隐私橡皮/图片瘦身器/
//! 视频转码农场/字幕工坊/媒体元数据中心/壁纸视频引擎/缩略图交响乐/
//! 色彩配置嵌入/媒体降级阶梯/损坏媒体救援/批量水印台/媒体预览蜂窝/
//! 大文件分块流/媒体回归金样/媒体年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F251 — 图像解码矩阵：格式能力总表
// ===========================================================================

/// 单个格式的编解码能力（解码/编码/无损）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DecodeCap {
    pub name: &'static str,
    pub decode: bool,
    pub encode: bool,
    pub lossless: bool,
}

pub const DECODE_MATRIX: [DecodeCap; 6] = [
    DecodeCap { name: "png", decode: true, encode: true, lossless: true },
    DecodeCap { name: "jpeg", decode: true, encode: true, lossless: false },
    DecodeCap { name: "gif", decode: true, encode: false, lossless: true },
    DecodeCap { name: "bmp", decode: true, encode: true, lossless: true },
    DecodeCap { name: "webp", decode: true, encode: true, lossless: false },
    DecodeCap { name: "qoi", decode: true, encode: true, lossless: true },
];

pub fn decode_supported(name: &str) -> bool {
    DECODE_MATRIX.iter().any(|c| c.name == name && c.decode)
}

pub fn is_lossless(name: &str) -> bool {
    DECODE_MATRIX.iter().any(|c| c.name == name && c.lossless)
}

// ===========================================================================
// F252 — GPU 编解码直通：帧带宽预算
// ===========================================================================

/// 每帧字节数 = w*h*bpp/8；直通条件：不超预算且尺寸为 2 的倍数。
pub fn gpu_passthrough_ok(w: u32, h: u32, bpp: u32, budget_bytes: u64) -> bool {
    w % 2 == 0 && h % 2 == 0 && (w as u64 * h as u64 * bpp as u64 / 8) <= budget_bytes
}

// ===========================================================================
// F253 — 格式先锋队：魔数嗅探
// ===========================================================================

/// 按文件头魔数判定格式（png/jpeg/gif/bmp/webp/qoi）。
pub fn sniff_format(data: &[u8]) -> Option<&'static str> {
    if data.len() >= 8 && data[0] == 0x89 && data[1] == b'P' && data[2] == b'N' && data[3] == b'G' {
        return Some("png");
    }
    if data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
        return Some("jpeg");
    }
    if data.len() >= 4 && &data[..4] == b"GIF8" {
        return Some("gif");
    }
    if data.len() >= 2 && data[0] == b'B' && data[1] == b'M' {
        return Some("bmp");
    }
    if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return Some("webp");
    }
    if data.len() >= 4 && &data[..4] == b"qoif" {
        return Some("qoi");
    }
    None
}

// ===========================================================================
// F254 — 截图工坊：区域与缩放参数
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ScreenshotJob {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub scale_permille: u16,
}

impl ScreenshotJob {
    pub fn valid(&self, screen_w: u32, screen_h: u32) -> bool {
        self.w > 0
            && self.h > 0
            && self.x + self.w <= screen_w
            && self.y + self.h <= screen_h
            && self.scale_permille >= 100
            && self.scale_permille <= 400
    }
    /// 输出像素（定点缩放）。
    pub fn out_px(&self) -> u64 {
        self.w as u64 * self.scale_permille as u64 / 1000
    }
}

// ===========================================================================
// F255 — 录屏导演台：码率计划
// ===========================================================================

pub const RECORD_BASE_KBPS: u32 = 3000;

/// 码率 = 基准 × fps × 质量‰，fps 上限 60。
pub fn record_bitrate_kbps(fps: u32, quality_permille: u16) -> Option<u32> {
    if fps == 0 || fps > 60 || quality_permille < 100 || quality_permille > 1000 {
        return None;
    }
    Some(RECORD_BASE_KBPS * fps * quality_permille as u32 / 1000)
}

// ===========================================================================
// F256 — 屏幕直播管线：端到端延迟预算
// ===========================================================================

pub const LATENCY_STAGES: [(&str, u32); 5] =
    [("capture", 40), ("encode", 60), ("net", 200), ("decode", 40), ("render", 16)];
pub const LIVE_LATENCY_BUDGET_MS: u32 = 1500;

pub fn pipeline_latency_ms() -> u32 {
    LATENCY_STAGES.iter().map(|(_, ms)| *ms).sum()
}

pub fn latency_ok() -> bool {
    pipeline_latency_ms() <= LIVE_LATENCY_BUDGET_MS
}

// ===========================================================================
// F257 — 相册时间轴：时间分桶
// ===========================================================================

/// 按天分桶（86400 秒/桶）。
pub fn day_bucket(ts_secs: u64) -> u64 {
    ts_secs / 86_400
}

/// 按 30 天近似月分桶。
pub fn month_bucket(ts_secs: u64) -> u64 {
    ts_secs / 2_592_000
}

// ===========================================================================
// F258 — 智能相册整理：场景评分归类
// ===========================================================================

#[derive(Clone, Copy)]
pub struct PhotoScore {
    pub portrait: u8,
    pub scenery: u8,
    pub night: u8,
    pub food: u8,
}

impl PhotoScore {
    /// 得分 ≥50 的最高分场景归入对应相册，均不过线则入"未分类"。
    pub fn best_album(&self) -> &'static str {
        let cands = [("portrait", self.portrait), ("scenery", self.scenery), ("night", self.night), ("food", self.food)];
        let mut best = ("uncategorized", 49u8);
        for &(name, score) in cands.iter() {
            if score >= 50 && score > best.1 {
                best = (name, score);
            }
        }
        best.0
    }
}

// ===========================================================================
// F259 — 人脸聚簇：距离阈值聚合
// ===========================================================================

/// 两张人脸距离（permille）小于阈值即同簇。
pub fn same_cluster(dist_permille: u16, threshold_permille: u16) -> bool {
    dist_permille < threshold_permille
}

/// 给定每张人脸的簇标签，统计不同簇数（255 表示未分配）。
pub fn distinct_clusters(labels: &[u8]) -> usize {
    let mut seen = [false; 255];
    let mut n = 0usize;
    for &l in labels {
        if (l as usize) < 255 && !seen[l as usize] {
            seen[l as usize] = true;
            n += 1;
        }
    }
    n
}

// ===========================================================================
// F260 — 图像搜索：感知哈希汉明距离
// ===========================================================================

pub fn hamming64(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// 汉明距离 ≤10 视为视觉相似。
pub fn visually_similar(a: u64, b: u64) -> bool {
    hamming64(a, b) <= 10
}

// ===========================================================================
// F261 — EXIF 隐私橡皮：敏感标签清单
// ===========================================================================

pub const SENSITIVE_TAGS: [&str; 6] =
    ["GPS", "SerialNumber", "CameraOwner", "Thumbnail", "Software", "Comment"];

pub fn is_sensitive_tag(tag: &str) -> bool {
    SENSITIVE_TAGS.contains(&tag)
}

/// 清除计划：保留数 = 总数 − 敏感数。
pub fn strip_plan(total_tags: u32, sensitive_tags: u32) -> u32 {
    total_tags.saturating_sub(sensitive_tags)
}

// ===========================================================================
// F262 — 图片瘦身器：目标体积与质量阶梯
// ===========================================================================

/// 目标字节数 = 原始 × 目标‰。
pub fn slim_target_bytes(orig: u64, target_permille: u16) -> u64 {
    orig * target_permille as u64 / 1000
}

/// 按目标比例选质量档：≤40% 用 40，≤70% 用 60，其余 80。
pub fn slim_quality(target_permille: u16) -> u8 {
    if target_permille <= 400 {
        40
    } else if target_permille <= 700 {
        60
    } else {
        80
    }
}

// ===========================================================================
// F263 — 视频转码农场：任务分片
// ===========================================================================

/// workers 轮次 = ceil(总帧数 / 每工位帧数)。
pub fn farm_rounds(total_frames: u64, frames_per_worker: u64) -> u64 {
    if frames_per_worker == 0 {
        return 0;
    }
    (total_frames + frames_per_worker - 1) / frames_per_worker
}

// ===========================================================================
// F264 — 字幕工坊：时间轴与行长
// ===========================================================================

#[derive(Clone, Copy)]
pub struct SubCue {
    pub start_ms: u32,
    pub end_ms: u32,
    pub text_len: u8,
}

impl SubCue {
    pub fn valid(&self) -> bool {
        self.end_ms > self.start_ms && self.text_len > 0 && self.text_len <= 42
    }
    pub fn overlaps(&self, other: &SubCue) -> bool {
        self.start_ms < other.end_ms && other.start_ms < self.end_ms
    }
}

// ===========================================================================
// F265 — 媒体元数据中心：元数据完备性
// ===========================================================================

#[derive(Clone, Copy)]
pub struct MediaMeta {
    pub title_len: u8,
    pub dur_ms: u32,
    pub w: u16,
    pub h: u16,
    pub has_icc: bool,
}

impl MediaMeta {
    pub fn complete(&self) -> bool {
        self.title_len > 0 && self.dur_ms > 0 && self.w > 0 && self.h > 0 && self.has_icc
    }
}

// ===========================================================================
// F266 — 壁纸视频引擎：循环预算
// ===========================================================================

pub const WALLPAPER_MAX_DUR_S: u32 = 10;
pub const WALLPAPER_MAX_FPS: u32 = 30;

pub fn wallpaper_video_ok(dur_s: u32, fps: u32) -> bool {
    dur_s > 0 && dur_s <= WALLPAPER_MAX_DUR_S && fps >= 1 && fps <= WALLPAPER_MAX_FPS
}

// ===========================================================================
// F267 — 缩略图交响乐：尺寸阶梯
// ===========================================================================

pub const THUMB_LADDER: [u32; 4] = [32, 128, 256, 512];

/// 取 ≥ 视图尺寸的最小档；超出则用最大档。
pub fn thumb_size_for(view_px: u32) -> u32 {
    for &s in THUMB_LADDER.iter() {
        if s >= view_px {
            return s;
        }
    }
    THUMB_LADDER[THUMB_LADDER.len() - 1]
}

// ===========================================================================
// F268 — 色彩配置嵌入：ICC 档案存在性
// ===========================================================================

/// 头部标签为 "ICC " 且档案 id 非零视为已嵌入。
pub fn icc_embedded(tag: &[u8; 4], profile_id: u64) -> bool {
    *tag == *b"ICC " && profile_id != 0
}

// ===========================================================================
// F269 — 媒体降级阶梯：内存分级
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaTier {
    Full,    // ≥4 GiB
    Light,   // ≥1 GiB
    Minimal, // 其余
}

pub fn media_tier(mem_mib: u32) -> MediaTier {
    if mem_mib >= 4096 {
        MediaTier::Full
    } else if mem_mib >= 1024 {
        MediaTier::Light
    } else {
        MediaTier::Minimal
    }
}

// ===========================================================================
// F270 — 损坏媒体救援：分块恢复判定
// ===========================================================================

/// 恢复 permille = 恢复块/总块 ×1000。
pub fn salvage_permille(recovered: u32, total: u32) -> u16 {
    if total == 0 {
        return 0;
    }
    (recovered.min(total) as u32 * 1000 / total) as u16
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SalvageVerdict {
    Full,     // ≥900‰
    Partial,  // ≥200‰
    Hopeless, // 其余
}

pub fn salvage_verdict(p: u16) -> SalvageVerdict {
    if p >= 900 {
        SalvageVerdict::Full
    } else if p >= 200 {
        SalvageVerdict::Partial
    } else {
        SalvageVerdict::Hopeless
    }
}

// ===========================================================================
// F271 — 批量水印台：九宫格锚点定位
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WmAnchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

/// 计算水印左上角坐标（边距 = min(w,h) × permille/1000）。
pub fn watermark_pos(w: u32, h: u32, wm_w: u32, wm_h: u32, anchor: WmAnchor, margin_permille: u16) -> (u32, u32) {
    let margin = (w.min(h) as u32 * margin_permille as u32 / 1000) as u32;
    match anchor {
        WmAnchor::TopLeft => (margin, margin),
        WmAnchor::TopRight => (w.saturating_sub(wm_w + margin), margin),
        WmAnchor::BottomLeft => (margin, h.saturating_sub(wm_h + margin)),
        WmAnchor::BottomRight => (w.saturating_sub(wm_w + margin), h.saturating_sub(wm_h + margin)),
        WmAnchor::Center => (w / 2 - wm_w / 2, h / 2 - wm_h / 2),
    }
}

// ===========================================================================
// F272 — 媒体预览蜂窝：网格列数
// ===========================================================================

/// 列数随条目数增长，夹在 2..=6。
pub fn grid_cols(items: usize) -> u8 {
    let cols = match items {
        0 => 0,
        1..=4 => 2,
        5..=12 => 3,
        13..=30 => 4,
        31..=60 => 5,
        _ => 6,
    };
    cols as u8
}

/// 蜂窝格位总数（用于补齐占位）。
pub fn grid_cells(items: usize) -> usize {
    let cols = grid_cols(items) as usize;
    if cols == 0 {
        return 0;
    }
    let rows = (items + cols - 1) / cols;
    rows * cols
}

// ===========================================================================
// F273 — 大文件分块流：分块与进度
// ===========================================================================

pub fn chunks_needed(total: u64, chunk: u64) -> u64 {
    if chunk == 0 {
        return 0;
    }
    (total + chunk - 1) / chunk
}

/// 流式进度 permille（total==0 视为 1000‰ 完成）。
pub fn stream_progress_permille(done: u64, total: u64) -> u16 {
    if total == 0 {
        return 1000;
    }
    ((done.min(total) * 1000 / total) as u16).min(1000)
}

// ===========================================================================
// F274 — 媒体回归金样：逐字节金样比对
// ===========================================================================

pub fn golden_match(actual: &[u8], golden: &[u8]) -> bool {
    actual == golden
}

// ===========================================================================
// F275 — 媒体年报：年度统计就绪判定
// ===========================================================================

pub const MEDIA_REPORT_SECTIONS: [&str; 4] = ["counts", "storage", "top-albums", "rescued"];

#[derive(Clone, Copy)]
pub struct MediaYearStats {
    pub photos: u32,
    pub videos: u32,
    pub gb_used: u32,
    pub rescued_files: u32,
}

impl MediaYearStats {
    pub fn report_ready(&self) -> bool {
        self.photos + self.videos > 0 && MEDIA_REPORT_SECTIONS.len() == 4
    }
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600media_checks() -> CheckSet {
    let mut set = CheckSet::new("m600media");

    // F251 图像解码矩阵
    set.add("F251 decode matrix", DECODE_MATRIX.len() == 6 && decode_supported("png") && !decode_supported("avif"), "six formats");
    set.add("F251 lossless flag", is_lossless("png") && !is_lossless("jpeg"), "lossless only");

    // F252 GPU 直通
    set.add("F252 gpu passthrough", gpu_passthrough_ok(1920, 1080, 32, 10_000_000), "within budget");
    set.add("F252 gpu budget", !gpu_passthrough_ok(7680, 4320, 32, 10_000_000) && !gpu_passthrough_ok(1921, 1080, 32, 99_999_999), "budget+align");

    // F253 格式嗅探
    set.add("F253 sniff png", sniff_format(&[0x89, b'P', b'N', b'G', 0, 0, 0, 0]) == Some("png"), "png magic");
    set.add("F253 sniff jpeg", sniff_format(&[0xFF, 0xD8, 0xFF, 0xE0]) == Some("jpeg"), "jpeg magic");
    set.add("F253 sniff webp", sniff_format(b"RIFFxxxxWEBPVP8 ") == Some("webp"), "webp magic");
    set.add("F253 sniff unknown", sniff_format(&[0, 1, 2, 3]).is_none(), "unknown");

    // F254 截图工坊
    let shot = ScreenshotJob { x: 10, y: 20, w: 100, h: 200, scale_permille: 200 };
    set.add("F254 screenshot valid", shot.valid(1920, 1080) && shot.out_px() == 20, "region+scale");
    set.add(
        "F254 screenshot bounds",
        !ScreenshotJob { x: 1900, y: 0, w: 100, h: 50, scale_permille: 100 }.valid(1920, 1080),
        "out of screen",
    );

    // F255 录屏导演台
    set.add("F255 record bitrate", record_bitrate_kbps(60, 500) == Some(90_000), "60fps@50%");
    set.add("F255 record guard", record_bitrate_kbps(0, 500).is_none() && record_bitrate_kbps(90, 500).is_none(), "fps cap");

    // F256 直播管线
    set.add("F256 live latency", latency_ok() && pipeline_latency_ms() == 356, "356ms total");

    // F257 相册时间轴
    set.add("F257 timeline buckets", day_bucket(172_800) == 2 && month_bucket(2_592_000) == 1, "day/month");

    // F258 智能相册
    let ps = PhotoScore { portrait: 90, scenery: 70, night: 20, food: 10 };
    set.add(
        "F258 smart album",
        ps.best_album() == "portrait"
            && PhotoScore { portrait: 10, scenery: 30, night: 20, food: 40 }.best_album() == "uncategorized",
        "threshold 50",
    );

    // F259 人脸聚簇
    set.add(
        "F259 face cluster",
        same_cluster(120, 200) && !same_cluster(300, 200) && distinct_clusters(&[1, 1, 2, 255, 2]) == 2,
        "threshold+labels",
    );

    // F260 图像搜索
    set.add("F260 hamming", hamming64(0b1010, 0b0011) == 2, "distance");
    set.add("F260 similar", visually_similar(0xFFFF_FFFF, 0xFFFF_FFFF) && !visually_similar(0, u64::MAX), "pHash");

    // F261 EXIF 橡皮
    set.add("F261 exif privacy", is_sensitive_tag("GPS") && !is_sensitive_tag("Orientation"), "sensitive list");
    set.add("F261 strip plan", strip_plan(20, 6) == 14 && strip_plan(3, 6) == 0, "kept count");

    // F262 图片瘦身
    set.add("F262 slim target", slim_target_bytes(4_000_000, 250) == 1_000_000, "250‰ target");
    set.add("F262 slim quality", slim_quality(300) == 40 && slim_quality(600) == 60 && slim_quality(900) == 80, "quality ladder");

    // F263 转码农场
    set.add("F263 transcode farm", farm_rounds(1001, 100) == 11 && farm_rounds(0, 100) == 0, "rounds");

    // F264 字幕工坊
    let s1 = SubCue { start_ms: 1000, end_ms: 2500, text_len: 20 };
    let s2 = SubCue { start_ms: 2000, end_ms: 3000, text_len: 30 };
    let s3 = SubCue { start_ms: 4000, end_ms: 5000, text_len: 0 };
    set.add("F264 subtitle cues", s1.valid() && !s3.valid(), "validity");
    set.add("F264 subtitle overlap", s1.overlaps(&s2) && !s1.overlaps(&s3), "no clash");

    // F265 元数据中心
    let meta = MediaMeta { title_len: 8, dur_ms: 90_000, w: 1920, h: 1080, has_icc: true };
    set.add("F265 media meta", meta.complete() && !MediaMeta { title_len: 0, dur_ms: 1, w: 1, h: 1, has_icc: false }.complete(), "completeness");

    // F266 壁纸视频
    set.add("F266 wallpaper video", wallpaper_video_ok(8, 30) && !wallpaper_video_ok(12, 30) && !wallpaper_video_ok(5, 90), "budget");

    // F267 缩略图
    set.add(
        "F267 thumbnails",
        thumb_size_for(16) == 32 && thumb_size_for(200) == 256 && thumb_size_for(999) == 512,
        "ladder",
    );

    // F268 色彩配置
    set.add("F268 icc embed", icc_embedded(b"ICC ", 0xDEAD_BEEF) && !icc_embedded(b"ICC ", 0) && !icc_embedded(b"gAMA", 7), "profile id");

    // F269 降级阶梯
    set.add(
        "F269 media tier",
        media_tier(8192) == MediaTier::Full && media_tier(2048) == MediaTier::Light && media_tier(512) == MediaTier::Minimal,
        "mem tiers",
    );

    // F270 损坏救援
    set.add("F270 salvage permille", salvage_permille(95, 100) == 950 && salvage_permille(0, 0) == 0, "recover ratio");
    set.add(
        "F270 salvage verdict",
        salvage_verdict(950) == SalvageVerdict::Full
            && salvage_verdict(300) == SalvageVerdict::Partial
            && salvage_verdict(50) == SalvageVerdict::Hopeless,
        "verdicts",
    );

    // F271 批量水印
    let (wx, wy) = watermark_pos(1000, 800, 100, 50, WmAnchor::BottomRight, 50);
    set.add("F271 watermark br", wx == 860 && wy == 710, "bottom-right");
    let (cx, cy) = watermark_pos(1000, 800, 100, 50, WmAnchor::Center, 0);
    set.add("F271 watermark center", cx == 450 && cy == 375, "centered");

    // F272 预览蜂窝
    set.add("F272 honeycomb cols", grid_cols(6) == 3 && grid_cols(40) == 5 && grid_cols(1) == 2, "column growth");
    set.add("F272 honeycomb cells", grid_cells(7) == 9 && grid_cells(0) == 0, "cell padding");

    // F273 分块流
    set.add("F273 chunk stream", chunks_needed(10_000, 4_096) == 3 && stream_progress_permille(500, 1_000) == 500, "chunks+progress");
    set.add("F273 stream done", stream_progress_permille(7, 0) == 1000 && stream_progress_permille(2_000, 1_000) == 1000, "clamped");

    // F274 金样回归
    set.add("F274 golden sample", golden_match(b"abc", b"abc") && !golden_match(b"abc", b"abd"), "byte equal");

    // F275 媒体年报
    let stats = MediaYearStats { photos: 1200, videos: 80, gb_used: 220, rescued_files: 15 };
    set.add(
        "F275 media report",
        stats.report_ready() && MEDIA_REPORT_SECTIONS.len() == 4 && !MediaYearStats { photos: 0, videos: 0, gb_used: 0, rescued_files: 0 }.report_ready(),
        "sections+counts",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f253_sniff_magics() {
        assert_eq!(sniff_format(b"GIF89a"), Some("gif"));
        assert_eq!(sniff_format(b"BMxx"), Some("bmp"));
        assert_eq!(sniff_format(b"qoif"), Some("qoi"));
        assert_eq!(sniff_format(b"nope"), None);
    }

    #[test]
    fn f255_bitrate_math() {
        assert_eq!(record_bitrate_kbps(30, 1000), Some(90_000));
        assert_eq!(record_bitrate_kbps(1, 100), Some(300));
        assert!(record_bitrate_kbps(61, 1000).is_none());
    }

    #[test]
    fn f267_thumb_ladder_edges() {
        assert_eq!(thumb_size_for(0), 32);
        assert_eq!(thumb_size_for(32), 32);
        assert_eq!(thumb_size_for(129), 256);
        assert_eq!(thumb_size_for(512), 512);
    }

    #[test]
    fn f270_salvage_boundaries() {
        assert_eq!(salvage_permille(900, 1000), 900);
        assert_eq!(salvage_verdict(899), SalvageVerdict::Partial);
        assert_eq!(salvage_verdict(199), SalvageVerdict::Hopeless);
        assert_eq!(salvage_permille(120, 100), 1000); // clamp
    }

    #[test]
    fn f271_watermark_anchors() {
        let (x, y) = watermark_pos(1000, 800, 100, 50, WmAnchor::TopLeft, 100);
        assert_eq!((x, y), (80, 80)); // min(1000,800)×100‰ = 80
        let (x, y) = watermark_pos(1000, 800, 100, 50, WmAnchor::TopRight, 100);
        assert_eq!((x, y), (820, 80));
    }

    #[test]
    fn f273_chunk_math() {
        assert_eq!(chunks_needed(4096, 4096), 1);
        assert_eq!(chunks_needed(4097, 4096), 2);
        assert_eq!(chunks_needed(100, 0), 0);
        assert_eq!(stream_progress_permille(0, 1000), 0);
    }

    #[test]
    fn f275_media_domain_selfcheck_all_pass() {
        let set = run_m600media_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
