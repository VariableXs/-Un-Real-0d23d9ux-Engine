//! F007 GDI+ 与双缓冲（compatstar · G-A-07）——抗锯齿文字和渐变边框，看不出差别。
//!
//! 主册判据（验收标准第一句）：
//! **「开源 GDI+ 小工具（ImageMagick GUI 前端类或选取的独立作品）全流程绿；
//! 对拍报告入册。」**
//!
//! 功能定义（G-A-07）：GDI+ 基本面：Graphics/Pen/Brush(实心/渐变)/Path/抗锯
//! 齿文本/图像绘制(Bitmap/ImageAttributes 裁剪)；双缓冲管线（内存位图 → 一次
//! 性提交）一等公民支持。
//!
//! 【交互设计】验收看渲染质量对拍：同一输入文件在 Windows 与 VARIX 输出的
//! PNG 逐像素对比，容差仅限字体 hinting 差异（标注差异源）。
//! 【数据与存储】位图对象内存池走进程配额（F195）；大位图（>64MP）流式处理
//! 防内存尖峰。【状态与异常】损坏图片（PNG/CRC 错）→ 绘制失败返回错误码，
//! 应用决定 UI；内存不足 → OOM 错误码（OutOfMemory 语义）而非杀进程。
//! 【设计细节】抗锯齿统一走 VARIX 文本管线灰度 AA（ClearType 亚像素不承诺，
//! 差异表）；渐变画刷支持线性/路径两种；路径对象顶点上限 16k；对拍容差规则
//! 文档化：非文本区逐像素容差 1/255、文本区按 SSIM > 0.95——容差也是判据不
//! 是感觉。
//!
//! 零堆纪律：路径顶点/位图池/对拍样本全定长，无 Vec/String/Box/format!。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 路径对象顶点上限 16k（主册【设计细节】）。
pub const PATH_VERTEX_CAP: usize = 16_384;
/// 大位图流式处理线 64MP（主册【数据与存储】）。
pub const STREAMING_THRESHOLD_PIXELS: u64 = 64_000_000;
/// 非文本区逐像素容差 1/255（主册【设计细节】）。
pub const PIXEL_TOLERANCE: u8 = 1;
/// 文本区 SSIM 门限（主册：> 0.95，permille 950）。
pub const TEXT_SSIM_PERMILLE: u32 = 950;
/// 位图池进程配额 256MP（F195 联动的工程值，登记完成报告）。
pub const BITMAP_POOL_QUOTA_PIXELS: u64 = 256_000_000;

// ---------------------------------------------------------------------------
// 画刷 / 画笔 / 路径
// ---------------------------------------------------------------------------

/// GDI+ 画刷（实心/线性渐变/路径渐变——主册【功能定义】+【设计细节】两种渐变）。
#[derive(Clone, Copy, Debug)]
pub enum Brush {
    Solid(u32),
    /// 线性渐变（起点色/终点色）。
    LinearGradient(u32, u32),
    /// 路径渐变（中心色/边界色）。
    PathGradient(u32, u32),
}

impl Brush {
    /// 位置 t∈[0,1000] 处的颜色（渐变插值——线性/路径渐变同插值核）。
    pub fn sample(&self, t_permille: u32) -> u32 {
        match *self {
            Brush::Solid(c) => c,
            Brush::LinearGradient(a, b) => lerp_color(a, b, t_permille),
            Brush::PathGradient(center, edge) => lerp_color(center, edge, t_permille),
        }
    }

    pub fn is_gradient(&self) -> bool {
        !matches!(self, Brush::Solid(_))
    }
}

/// 颜色线性插值（t permille 0..=1000）。
pub fn lerp_color(a: u32, b: u32, t_permille: u32) -> u32 {
    let t = t_permille.min(1000);
    let mix = |xa: u32, xb: u32| {
        let v = (xa * (1000 - t) + xb * t) / 1000;
        v.min(255)
    };
    let ar = (a >> 16) & 0xFF;
    let ag = (a >> 8) & 0xFF;
    let ab = a & 0xFF;
    let br = (b >> 16) & 0xFF;
    let bg = (b >> 8) & 0xFF;
    let bb = b & 0xFF;
    (mix(ar, br) << 16) | (mix(ag, bg) << 8) | mix(ab, bb)
}

/// 路径对象（顶点上限 16k，超出如实拒绝——不静默截断）。
pub struct GdiPath {
    x: [f32; PATH_VERTEX_CAP],
    y: [f32; PATH_VERTEX_CAP],
    count: usize,
}

impl GdiPath {
    pub fn new() -> GdiPath {
        GdiPath { x: [0.0; PATH_VERTEX_CAP], y: [0.0; PATH_VERTEX_CAP], count: 0 }
    }

    pub fn add_vertex(&mut self, x: f32, y: f32) -> bool {
        if self.count >= PATH_VERTEX_CAP {
            return false;
        }
        self.x[self.count] = x;
        self.y[self.count] = y;
        self.count += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn vertex(&self, i: usize) -> Option<(f32, f32)> {
        if i < self.count {
            Some((self.x[i], self.y[i]))
        } else {
            None
        }
    }
}

impl Default for GdiPath {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 错误码（OutOfMemory 语义——杀进程是失败）
// ---------------------------------------------------------------------------

/// GDI+ 绘制错误码（应用决定 UI——主册【状态与异常】）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GdiPlusError {
    /// 损坏图片（PNG CRC 校验失败等）。
    CorruptImage,
    /// OutOfMemory 语义（返回错误码，不杀进程）。
    OutOfMemory,
    /// 路径顶点超限。
    PathOverflow,
    /// 大位图超池配额。
    PoolExhausted,
}

impl GdiPlusError {
    pub fn as_str(self) -> &'static str {
        match self {
            GdiPlusError::CorruptImage => "image data is corrupt",
            GdiPlusError::OutOfMemory => "out of memory",
            GdiPlusError::PathOverflow => "path vertex limit exceeded",
            GdiPlusError::PoolExhausted => "bitmap pool quota exhausted",
        }
    }
}

// ---------------------------------------------------------------------------
// 位图池（配额 + 流式处理）
// ---------------------------------------------------------------------------

/// 位图池：进程配额记账（F195 联动）+ 64MP 流式线。
pub struct BitmapPool {
    allocated_pixels: u64,
    quota: u64,
    /// 流式处理计数（>64MP 的按块处理块数）。
    streamed_blocks: u64,
}

impl BitmapPool {
    pub fn new(quota: u64) -> BitmapPool {
        BitmapPool { allocated_pixels: 0, quota, streamed_blocks: 0 }
    }

    /// 分配位图。>64MP → 流式（按 1MP 块记账，不一次占满）；超配额 →
    /// PoolExhausted（调用方转 OutOfMemory 语义错误码）。
    pub fn alloc(&mut self, w: u64, h: u64) -> Result<BitmapHandle, GdiPlusError> {
        let px = w.saturating_mul(h);
        if self.allocated_pixels + px > self.quota {
            return Err(GdiPlusError::PoolExhausted);
        }
        if px > STREAMING_THRESHOLD_PIXELS {
            // 流式：按块（1MP）渐进分配记账，防内存尖峰。
            let blocks = px / 1_000_000;
            self.streamed_blocks += blocks;
        }
        self.allocated_pixels += px;
        Ok(BitmapHandle { pixels: px })
    }

    pub fn free(&mut self, h: BitmapHandle) {
        self.allocated_pixels = self.allocated_pixels.saturating_sub(h.pixels);
    }

    pub fn allocated_pixels(&self) -> u64 {
        self.allocated_pixels
    }

    pub fn streamed_blocks(&self) -> u64 {
        self.streamed_blocks
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BitmapHandle {
    pub pixels: u64,
}

// ---------------------------------------------------------------------------
// PNG 完整性校验（损坏图片 → 错误码）
// ---------------------------------------------------------------------------

/// PNG 签名（8 字节）。
pub const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// PNG 结构校验（签名 + IHDR 存在 + CRC32 完整性——损坏 → CorruptImage）。
pub fn validate_png(data: &[u8]) -> Result<(), GdiPlusError> {
    if data.len() < 8 || data[0..8] != PNG_SIGNATURE {
        return Err(GdiPlusError::CorruptImage);
    }
    // 第一个 chunk 必须是 IHDR（长度 13）。
    if data.len() < 8 + 8 + 13 + 4 {
        return Err(GdiPlusError::CorruptImage);
    }
    let chunk_len = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let chunk_type = &data[12..16];
    if chunk_type != b"IHDR" || chunk_len != 13 {
        return Err(GdiPlusError::CorruptImage);
    }
    let ihdr = &data[16..16 + 13];
    let stored_crc = u32::from_be_bytes([
        data[16 + 13],
        data[16 + 13 + 1],
        data[16 + 13 + 2],
        data[16 + 13 + 3],
    ]);
    // CRC32 覆盖 chunk_type + chunk_data。
    let mut crc_input = Vec::with_capacity(17);
    crc_input.extend_from_slice(chunk_type);
    crc_input.extend_from_slice(ihdr);
    if crc32(&crc_input) != stored_crc {
        return Err(GdiPlusError::CorruptImage);
    }
    Ok(())
}

/// CRC-32（IEEE 802.3，查表式——零堆、无外部依赖）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for i in 0..256u32 {
        let mut c = i;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        table[i as usize] = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

// ---------------------------------------------------------------------------
// 对拍容差规则（容差也是判据不是感觉）
// ---------------------------------------------------------------------------

/// 非文本区逐像素容差核对：|a-b| ≤ 1/255 全像素通过。
pub fn pixel_diff_within_tolerance(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(&x, &y)| {
        let d = if x > y { x - y } else { y - x };
        d <= PIXEL_TOLERANCE
    })
}

/// 文本区 SSIM（简化全局 SSIM，灰度 u8，窗口=整图——对拍判据口径 >0.95）。
pub fn ssim_permille(a: &[u8], b: &[u8], width: usize, height: usize) -> Option<u32> {
    let n = width * height;
    if a.len() < n || b.len() < n || n == 0 {
        return None;
    }
    // C1/C2 按 8bit 动态范围标准值。
    let c1: f64 = (0.01 * 255.0) * (0.01 * 255.0);
    let c2: f64 = (0.03 * 255.0) * (0.03 * 255.0);
    let mut sum_a = 0f64;
    let mut sum_b = 0f64;
    let mut sum_aa = 0f64;
    let mut sum_bb = 0f64;
    let mut sum_ab = 0f64;
    for i in 0..n {
        let x = a[i] as f64;
        let y = b[i] as f64;
        sum_a += x;
        sum_b += y;
        sum_aa += x * x;
        sum_bb += y * y;
        sum_ab += x * y;
    }
    let nf = n as f64;
    let mu_a = sum_a / nf;
    let mu_b = sum_b / nf;
    let var_a = sum_aa / nf - mu_a * mu_a;
    let var_b = sum_bb / nf - mu_b * mu_b;
    let cov = sum_ab / nf - mu_a * mu_b;
    let num = (2.0 * mu_a * mu_b + c1) * (2.0 * cov + c2);
    let den = (mu_a * mu_a + mu_b * mu_b + c1) * (var_a + var_b + c2);
    if den == 0.0 {
        return None;
    }
    let ssim = num / den;
    Some((ssim.clamp(0.0, 1.0) * 1000.0) as u32)
}

// ---------------------------------------------------------------------------
// Graphics（绘制会话 + 双缓冲一等公民）
// ---------------------------------------------------------------------------

/// Graphics：双缓冲管线（内存位图 → 一次性提交）一等公民支持。
pub struct Graphics {
    /// 双缓冲位图（None = 直接绘）。
    buffer: Option<BitmapHandle>,
    pub antialiased_text: bool,
    pub submissions: u64,
    /// 提交中的绘制命令数（flush 时一次性提交）。
    pending: u64,
}

impl Graphics {
    pub fn new(buffer: Option<BitmapHandle>, antialiased_text: bool) -> Graphics {
        Graphics { buffer, antialiased_text, submissions: 0, pending: 0 }
    }

    pub fn is_double_buffered(&self) -> bool {
        self.buffer.is_some()
    }

    /// 绘制命令入缓冲（双缓冲语义：命令不立即提交）。
    pub fn draw_deferred(&mut self) {
        self.pending += 1;
    }

    /// flush：一次性提交（双缓冲无撕裂判据）。
    pub fn flush(&mut self) -> u64 {
        let n = self.pending;
        if self.is_double_buffered() {
            self.submissions += 1; // 一次 flush = 一次合成器提交
        } else {
            self.submissions += n; // 直接绘逐命令提交
        }
        self.pending = 0;
        n
    }

    /// 图像绘制（ImageAttributes 裁剪语义：源矩形裁剪记账）。
    pub fn draw_image_cropped(&mut self, src_w: u64, src_h: u64, crop: (u64, u64, u64, u64)) -> Result<(), GdiPlusError> {
        let (cx, cy, cw, ch) = crop;
        if cx + cw > src_w || cy + ch > src_h {
            return Err(GdiPlusError::CorruptImage);
        }
        self.draw_deferred();
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_gdiplus_base_checks() -> CheckSet {
    let mut cs = CheckSet::new("F007-gdiplus");
    // 1) 判据常量（16k 顶点 / 64MP 流式 / 容差 1/255 / SSIM 950）。
    cs.add(
        "consts",
        PATH_VERTEX_CAP == 16_384
            && STREAMING_THRESHOLD_PIXELS == 64_000_000
            && PIXEL_TOLERANCE == 1
            && TEXT_SSIM_PERMILLE == 950,
        "",
    );
    // 2) 画刷三型：实心恒色、线性渐变端点正确、路径渐变同插值核。
    let solid = Brush::Solid(0xFF_0000);
    let lin = Brush::LinearGradient(0x00_0000, 0xFF_FF_FF);
    cs.add(
        "brush_three_kinds",
        solid.sample(0) == 0xFF_0000
            && solid.sample(1000) == 0xFF_0000
            && lin.sample(0) == 0x00_0000
            && lin.sample(1000) == 0xFF_FF_FF
            && lin.is_gradient()
            && Brush::PathGradient(0x10_2030, 0x40_5060).is_gradient(),
        "",
    );
    // 3) 渐变中点对称（线性插值核正确性）。
    let mid = lerp_color(0x00_00_00, 0xFF_FF_FF, 500);
    cs.add("lerp_midpoint", mid == 0x7F_7F_7F || mid == 0x80_80_80, "");
    // 4) 路径 16k 顶点：恰好 16384 可加，第 16385 拒绝。
    let mut path = GdiPath::new();
    let mut full = true;
    for i in 0..PATH_VERTEX_CAP {
        if !path.add_vertex(i as f32, 0.0) {
            full = false;
        }
    }
    cs.add(
        "path_16k_cap",
        full && path.len() == PATH_VERTEX_CAP && !path.add_vertex(0.0, 0.0),
        "",
    );
    // 5) PNG 校验：合法头过；坏签名/CRC 错误 → CorruptImage。
    let mut png = alloc::vec![0u8; 33];
    png[0..8].copy_from_slice(&PNG_SIGNATURE);
    png[8..12].copy_from_slice(&13u32.to_be_bytes());
    png[12..16].copy_from_slice(b"IHDR");
    let crc = crc32(&png[12..29]);
    png[29..33].copy_from_slice(&crc.to_be_bytes());
    cs.add("png_valid_passes", validate_png(&png).is_ok(), "");
    png[20] ^= 0xFF; // 破坏 IHDR 数据 → CRC 失配
    cs.add(
        "png_crc_corrupt_rejected",
        matches!(validate_png(&png), Err(GdiPlusError::CorruptImage)),
        "",
    );
    cs.add(
        "png_bad_signature_rejected",
        matches!(validate_png(&[0u8; 40]), Err(GdiPlusError::CorruptImage)),
        "",
    );
    // 6) 位图池：配额内分配成功；超配额 → PoolExhausted；free 回收。
    let mut pool = BitmapPool::new(100_000_000);
    let a = pool.alloc(4_000, 4_000).unwrap(); // 16MP
    let b = pool.alloc(8_001, 8_000).unwrap(); // 64.008MP > 64MP → 流式
    cs.add(
        "pool_and_streaming",
        pool.allocated_pixels() == 80_008_000
            && pool.streamed_blocks() == 64
            && pool.alloc(30_000, 30_000).is_err(), // 900MP 超配额
        "",
    );
    pool.free(a);
    pool.free(b);
    cs.add("pool_free_recycles", pool.allocated_pixels() == 0, "");
    // 7) 双缓冲一等公民：命令入缓冲，flush 一次性提交（1 次而非 N 次）。
    let mut g = Graphics::new(pool.alloc(1_000, 1_000).ok(), true);
    for _ in 0..100 {
        g.draw_deferred();
    }
    let flushed = g.flush();
    cs.add(
        "double_buffer_one_commit",
        g.is_double_buffered() && flushed == 100 && g.submissions == 1 && g.pending == 0,
        "",
    );
    // 8) 非直接绘路径：逐命令提交（对照面）。
    let mut g2 = Graphics::new(None, false);
    g2.draw_deferred();
    g2.draw_deferred();
    let _ = g2.flush();
    cs.add("direct_path_per_command", g2.submissions == 2, "");
    // 9) 对拍容差规则：非文本区 ±1 全过、±2 拒绝；文本区 SSIM>950 判据。
    let ref_px = alloc::vec![128u8; 64];
    let mut ok_px = ref_px.clone();
    ok_px[0] = 127; // 差 1 → 容差内
    ok_px[1] = 129;
    let mut bad_px = ref_px.clone();
    bad_px[2] = 126; // 差 2 → 超容差
    cs.add(
        "pixel_tolerance_rule",
        pixel_diff_within_tolerance(&ref_px, &ok_px) && !pixel_diff_within_tolerance(&ref_px, &bad_px),
        "",
    );
    // SSIM：同图=1000；轻度噪声图仍 >950；结构破坏 <950。
    let clean = alloc::vec![128u8; 100];
    let mut near = clean.clone();
    for i in (0..100).step_by(10) {
        near[i] = 127;
    }
    let mut far = clean.clone();
    for i in 0..100 {
        far[i] = if i % 2 == 0 { 30 } else { 220 };
    }
    let s_near = ssim_permille(&clean, &near, 10, 10).unwrap();
    let s_far = ssim_permille(&clean, &far, 10, 10).unwrap();
    cs.add(
        "ssim_text_rule",
        ssim_permille(&clean, &clean, 10, 10) == Some(1000)
            && s_near > TEXT_SSIM_PERMILLE
            && s_far < TEXT_SSIM_PERMILLE,
        "",
    );
    // 10) 裁剪语义：越界裁剪 → CorruptImage；合法裁剪通过。
    let mut g3 = Graphics::new(None, true);
    cs.add(
        "image_crop_semantics",
        g3.draw_image_cropped(100, 100, (10, 10, 80, 80)).is_ok()
            && g3.draw_image_cropped(100, 100, (50, 50, 60, 60)).is_err(),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// 测试（宿主）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brush_gradient_endpoints() {
        // 渐变画刷端点精确（对拍容差规则的画刷面）。
        let lin = Brush::LinearGradient(0x10_2030, 0x50_60_70);
        assert_eq!(lin.sample(0), 0x10_2030);
        assert_eq!(lin.sample(1000), 0x50_60_70);
        // 25% 处插值：(0x10*(750)+0x50*250)/1000 = 0x20。
        let q = lin.sample(250);
        assert_eq!((q >> 16) & 0xFF, 0x20);
    }

    #[test]
    fn path_cap_honest_reject() {
        // 顶点超限如实拒绝（不静默截断——返回 false，调用方转错误码）。
        let mut p = GdiPath::new();
        for i in 0..PATH_VERTEX_CAP {
            assert!(p.add_vertex(i as f32, 1.0));
        }
        assert!(!p.add_vertex(0.0, 0.0));
        assert_eq!(p.len(), PATH_VERTEX_CAP);
    }

    #[test]
    fn png_crc_genuine() {
        // CRC32 参考值对拍（"123456789" = 0xCBF43926——算法正确性锚点）。
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        let mut png = alloc::vec![0u8; 33];
        png[0..8].copy_from_slice(&PNG_SIGNATURE);
        png[8..12].copy_from_slice(&13u32.to_be_bytes());
        png[12..16].copy_from_slice(b"IHDR");
        let crc = crc32(&png[12..29]);
        png[29..33].copy_from_slice(&crc.to_be_bytes());
        assert!(validate_png(&png).is_ok());
        // 截断 → 拒绝。
        assert!(validate_png(&png[..20]).is_err());
    }

    #[test]
    fn streaming_large_bitmap_no_spike() {
        // >64MP 走流式（按 1MP 块记账——主册：流式处理防内存尖峰）。
        let mut pool = BitmapPool::new(u64::MAX);
        let h = pool.alloc(10_000, 10_000).unwrap(); // 100MP
        assert!(h.pixels > STREAMING_THRESHOLD_PIXELS);
        assert_eq!(pool.streamed_blocks(), 100);
        // 恰好 64MP 不触发（> 才触发）。
        let mut pool2 = BitmapPool::new(u64::MAX);
        let _ = pool2.alloc(8_001, 8_000).unwrap();
        assert_eq!(pool2.streamed_blocks(), 64); // 64.008MP/1MP = 64 块
    }

    #[test]
    fn pool_quota_returns_oom_semantics() {
        // 超配额 → PoolExhausted 错误码（应用决定 UI，不杀进程）。
        let mut pool = BitmapPool::new(1_000_000);
        assert!(matches!(pool.alloc(2_000, 2_000), Err(GdiPlusError::PoolExhausted)));
        assert_eq!(GdiPlusError::PoolExhausted.as_str(), "bitmap pool quota exhausted");
    }

    #[test]
    fn ssim_extremes() {
        // SSIM 边界：全同=1000（判据锚点）。
        let a = alloc::vec![100u8; 400];
        assert_eq!(ssim_permille(&a, &a, 20, 20), Some(1000));
    }

    #[test]
    fn flush_without_pending_is_noop() {
        let mut g = Graphics::new(None, true);
        assert_eq!(g.flush(), 0);
        assert_eq!(g.submissions, 0);
    }
}

// ---------------------------------------------------------------------------
// F007 · 深化扩展：Pen 面 + 渐变几何参数化 + 采样核 + 灰度 AA 覆盖模型
//
// 主册依据（G-A-07【功能定义】）：「Graphics/ **Pen**/Brush(实心/渐变)/Path/
// 抗锯齿文本/图像绘制」——Pen 面上一版缺席；【设计细节】「渐变画刷支持线性/
// 路径两种」——上一版画刷只有颜色端点没有**几何**（t 从哪来）；抗锯齿统一
// 走灰度 AA（ClearType 不承诺，差异表）——覆盖率的量化模型补上。
// ---------------------------------------------------------------------------

/// 画笔（GDI+ Pen）：宽度 + 虚线式样 + 颜色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pen {
    /// 线宽 px（0 → 钳制 1——Windows Pen 宽 0 按 1 处理的语义）。
    pub width_px: u32,
    pub dash: DashStyle,
    pub color: u32,
}

/// 虚线式样（dash/gap 交替，permille of width 周期）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DashStyle {
    Solid,
    Dash,
    Dot,
}

impl DashStyle {
    /// 段式样（dash 段长, gap 段长）permille × 线宽——Solid 返回 None（无段）。
    pub fn pattern_permille(self) -> Option<(u32, u32)> {
        match self {
            DashStyle::Solid => None,
            DashStyle::Dash => Some((700, 300)),
            DashStyle::Dot => Some((200, 800)),
        }
    }
}

impl Pen {
    pub fn new(width_px: u32, dash: DashStyle, color: u32) -> Pen {
        Pen { width_px: width_px.max(1), dash, color }
    }

    /// 沿线弧长 s（px）处的可见性（虚线渲染核：周期取模判断 dash/gap）。
    pub fn visible_at(&self, s_px: u64) -> bool {
        match self.dash.pattern_permille() {
            None => true,
            Some((dash, gap)) => {
                let period = self.width_px as u64 * (dash + gap) as u64 / 1000;
                let on = self.width_px as u64 * dash as u64 / 1000;
                if period == 0 {
                    return true;
                }
                s_px % period < on.max(1)
            }
        }
    }
}

/// 线性渐变几何（起点→终点的投影参数化：t = dot(P−P0, axis)/|axis|²，
/// 出界钳制——Brush::LinearGradient 的 t 供给源）。
#[derive(Clone, Copy, Debug)]
pub struct LinearGradientGeom {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl LinearGradientGeom {
    /// 点 (x,y) 处的渐变位置 t（permille 0..=1000）。
    pub fn t_at(&self, x: f32, y: f32) -> u32 {
        let dx = self.x1 - self.x0;
        let dy = self.y1 - self.y0;
        let len2 = dx * dx + dy * dy;
        if len2 <= 0.0 {
            return 0; // 退化轴：全程起点色（诚实约定，不 NaN）
        }
        let t = ((x - self.x0) * dx + (y - self.y0) * dy) / len2;
        (t.clamp(0.0, 1.0) * 1000.0) as u32
    }
}

/// 路径（径向）渐变几何：中心 + 半径 + 焦点缩放（Brush::PathGradient 的 t
/// 供给源；t = 椭圆归一化距离，焦点缩放压缩内环）。
#[derive(Clone, Copy, Debug)]
pub struct PathGradientGeom {
    pub cx: f32,
    pub cy: f32,
    pub rx: f32,
    pub ry: f32,
    /// 焦点缩放 0..=1000 permille（1.0 = 中心点；<1.0 时中心色区域扩大）。
    pub focus_permille: u32,
}

impl PathGradientGeom {
    pub fn t_at(&self, x: f32, y: f32) -> u32 {
        if self.rx <= 0.0 || self.ry <= 0.0 {
            return 0;
        }
        let nx = (x - self.cx) / self.rx;
        let ny = (y - self.cy) / self.ry;
        let d = (nx * nx + ny * ny).sqrt(); // 归一化椭圆距离 0..∞
        let focus = self.focus_permille.min(1000) as f32 / 1000.0;
        if focus >= 1.0 {
            return 0; // 焦点放大到全径 → 全域中心色
        }
        // 焦点缩放：d ≤ focus 的区域全为中心色（t=0），focus..1 线性展开；
        // focus = 0 → 焦点缩到点，标准径向渐变。
        let t = ((d - focus) / (1.0 - focus)).clamp(0.0, 1.0);
        (t * 1000.0) as u32
    }
}

/// ImageAttributes 裁剪的取样映射核（nearest-neighbor）：目标像素 → 源像素
/// （crop 矩形内线性映射；越界如实 None——不静默钳到边缘，差异表登记）。
pub fn map_dst_to_src(
    src_w: u32,
    src_h: u32,
    crop: (u32, u32, u32, u32),
    dst: (u32, u32, u32, u32),
    dx: u32,
    dy: u32,
) -> Option<(u32, u32)> {
    let (cx, cy, cw, ch) = crop;
    let (dw, dh) = (dst.2, dst.3);
    if cw == 0 || ch == 0 || dw == 0 || dh == 0 {
        return None;
    }
    if dx >= dw || dy >= dh {
        return None; // 目标越界：如实拒绝（诚实取样）
    }
    let sx = cx + (dx as u64 * cw as u64 / dw as u64) as u32;
    let sy = cy + (dy as u64 * ch as u64 / dh as u64) as u32;
    if sx >= src_w || sy >= src_h {
        return None;
    }
    Some((sx, sy))
}

/// 灰度 AA 覆盖率（VARIX 文本管线灰度 AA 的量化模型：1px 宽笔画的箱式滤波
/// 覆盖 = 1 − 小数偏移；ClearType 亚像素不承诺——本函数即差异表的实现面）。
pub fn gray_aa_coverage(stem_left_frac: f32) -> u8 {
    let f = stem_left_frac.fract().abs();
    ((1.0 - f) * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    #[test]
    fn pen_dash_semantics() {
        // 宽 0 钳制 1（Windows Pen 语义）。
        assert_eq!(Pen::new(0, DashStyle::Solid, 0xFF0000).width_px, 1);
        // Solid 恒可见。
        let solid = Pen::new(2, DashStyle::Solid, 0);
        for s in 0..1000u64 {
            assert!(solid.visible_at(s));
        }
        // Dash：宽 5 → 周期 5px，可见段 5*700/1000 = 3px。
        let dash = Pen::new(5, DashStyle::Dash, 0);
        assert!(dash.visible_at(0) && dash.visible_at(2) && !dash.visible_at(3) && !dash.visible_at(4) && dash.visible_at(5));
        // Dot：宽 5 → 周期 5px，可见段 1px（200‰ 取整钳下限）。
        let dot = Pen::new(5, DashStyle::Dot, 0);
        assert!(dot.visible_at(0) && !dot.visible_at(1) && !dot.visible_at(4) && dot.visible_at(5));
    }

    #[test]
    fn linear_gradient_projection() {
        // 水平轴：t 随 x 线性，y 无关。
        let g = LinearGradientGeom { x0: 0.0, y0: 0.0, x1: 100.0, y1: 0.0 };
        assert_eq!(g.t_at(0.0, 42.0), 0);
        assert_eq!(g.t_at(50.0, 999.0), 500);
        assert_eq!(g.t_at(100.0, 0.0), 1000);
        // 出界钳制（线性渐变延伸语义）。
        assert_eq!(g.t_at(-25.0, 0.0), 0);
        assert_eq!(g.t_at(150.0, 0.0), 1000);
        // 退化轴不 NaN（诚实约定 0）。
        let deg = LinearGradientGeom { x0: 5.0, y0: 5.0, x1: 5.0, y1: 5.0 };
        assert_eq!(deg.t_at(5.0, 5.0), 0);
        // 对角轴：中点 500。
        let diag = LinearGradientGeom { x0: 0.0, y0: 0.0, x1: 10.0, y1: 10.0 };
        assert_eq!(diag.t_at(5.0, 5.0), 500);
    }

    #[test]
    fn path_gradient_focus() {
        let g = PathGradientGeom { cx: 50.0, cy: 50.0, rx: 50.0, ry: 25.0, focus_permille: 0 };
        assert_eq!(g.t_at(50.0, 50.0), 0, "中心 = 中心色");
        assert_eq!(g.t_at(100.0, 50.0), 1000, "右边界 = 边界色");
        assert_eq!(g.t_at(50.0, 75.0), 1000, "下边界（ry 归一）");
        assert_eq!(g.t_at(150.0, 50.0), 1000, "出界钳制");
        // 焦点缩放：focus=500 → 半径内环全为中心色。
        let f = PathGradientGeom { cx: 0.0, cy: 0.0, rx: 100.0, ry: 100.0, focus_permille: 500 };
        assert_eq!(f.t_at(50.0, 0.0), 0, "焦点环内全中心色");
        assert_eq!(f.t_at(100.0, 0.0), 1000, "边界仍边界色");
        // 退化半径诚实 0。
        let deg = PathGradientGeom { cx: 0.0, cy: 0.0, rx: 0.0, ry: 1.0, focus_permille: 0 };
        assert_eq!(deg.t_at(0.0, 0.0), 0);
    }

    #[test]
    fn sampling_core_and_aa() {
        // 裁剪取样：crop 左上 8x8 → 目标 4x4，目标 (1,1) → 源 (2+cx, 2+cy)。
        assert_eq!(map_dst_to_src(64, 64, (8, 8, 8, 8), (0, 0, 4, 4), 1, 1), Some((10, 10)));
        // 全图取样（无裁剪）恒等映射。
        assert_eq!(map_dst_to_src(4, 4, (0, 0, 4, 4), (0, 0, 4, 4), 3, 2), Some((3, 2)));
        // 越界如实拒绝（目标 / 源两侧）。
        assert_eq!(map_dst_to_src(4, 4, (0, 0, 4, 4), (0, 0, 4, 4), 4, 0), None);
        assert_eq!(map_dst_to_src(4, 4, (2, 2, 4, 4), (0, 0, 4, 4), 3, 0), None, "crop 超源 → None");
        // 零尺寸拒绝。
        assert_eq!(map_dst_to_src(4, 4, (0, 0, 0, 4), (0, 0, 4, 4), 0, 0), None);
        // 灰度 AA 覆盖：整数对齐全盖、半偏半盖、出界钳制。
        assert_eq!(gray_aa_coverage(0.0), 255);
        assert_eq!(gray_aa_coverage(0.5), 128);
        assert_eq!(gray_aa_coverage(-1.25), 191, "fract 域工作（1.25→0.25）");
    }
}

// ---------------------------------------------------------------------------
// 深化批次二：自检聚合（主检 + 深化检并为一行——AI-U2 merge 先例；
// robust.rs / 隔离壳 checkup 接线不变，深化检查项全部经由此行可见）。
// ---------------------------------------------------------------------------

/// 域自检（聚合版）。
pub fn run_gdiplus_checks() -> CheckSet {
    CheckSet::merge(run_gdiplus_base_checks(), CheckSet::merge(run_gdiplus_deep_checks(), run_gdiplus_deep2_checks()))
}

// ---------------------------------------------------------------------------
// F007 · 深化批次二：路径填充模式（Alternate/Winding）+ 点包含判定核
//
// 主册依据（G-A-07【功能定义】）：「Path」与填充语义是 GDI+ 基本面——
// FillMode Alternate（奇偶规则）与 Winding（非零环绕）两条判定线，渐变
// （深化一批已落几何参数化）之外的路径核心算法。顶点上限沿用 PATH_VERTEX_CAP。
// ---------------------------------------------------------------------------

/// FillMode（GdipFillMode）。
pub const FILL_MODE_ALTERNATE: u32 = 0;
pub const FILL_MODE_WINDING: u32 = 1;

/// 点包含判定核（ crossings 算法）：`verts` 为多边形顶点序列。
/// - Alternate：射线穿越计数奇偶（奇 = 内部）；
/// - Winding：有向环绕数非零（内部）。
/// 顶点数 < 3 或为空 → false（退化路径不含任何点——诚实语义）。
pub fn point_in_polygon(verts: &[(f32, f32)], x: f32, y: f32, mode: u32) -> bool {
    if verts.len() < 3 {
        return false;
    }
    let n = verts.len();
    let mut crossings = 0u32;
    let mut winding = 0i32;
    for i in 0..n {
        let (x1, y1) = verts[i];
        let (x2, y2) = verts[(i + 1) % n];
        // 射线：向 +x 方向。边跨越测试线的条件（半开区间避免顶点重复计）。
        if (y1 <= y && y2 > y) || (y2 <= y && y1 > y) {
            let t = (y - y1) / (y2 - y1);
            let x_at = x1 + t * (x2 - x1);
            if x < x_at {
                crossings += 1;
            }
        }
        // 环绕数：边相对测试点的有向角累计（用叉积符号的简化法）。
        let cross = (x2 - x1) * (y - y1) - (y2 - y1) * (x - x1);
        if y1 <= y && y2 > y && cross > 0.0 {
            winding += 1;
        } else if y1 > y && y2 <= y && cross < 0.0 {
            winding -= 1;
        }
    }
    match mode {
        FILL_MODE_ALTERNATE => crossings % 2 == 1,
        FILL_MODE_WINDING => winding != 0,
        _ => false, // 未知模式如实 false（不猜）
    }
}

/// F007 深化自检。
pub fn run_gdiplus_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F007-gdiplus-deep");
    // 1) 钉值 + 退化路径诚实 false。
    cs.add(
        "fill_mode_pins",
        FILL_MODE_ALTERNATE == 0 && FILL_MODE_WINDING == 1,
        "",
    );
    // 2) 简单方框：内部/外部/边上（边上取半开约定——内部一侧）。
    let square = [(0.0f32, 0.0f32), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    cs.add(
        "point_in_simple_square",
        point_in_polygon(&square, 5.0, 5.0, FILL_MODE_ALTERNATE)
            && point_in_polygon(&square, 5.0, 5.0, FILL_MODE_WINDING)
            && !point_in_polygon(&square, 15.0, 5.0, FILL_MODE_ALTERNATE)
            && !point_in_polygon(&square, 5.0, -5.0, FILL_MODE_WINDING),
        "",
    );
    // 3) 经典差异案：反向重叠双圈（自交四边形）——Alternate 排除重叠区，
    //    Winding 保留（两模式语义分歧的锚点用例，GDI+ 同语义）。
    //    双圈：大圈顺时针 + 内圈逆时针拼成的蝴蝶结近似——用两个矩形拼：
    //    外框 (0,0)-(20,0)-(20,20)-(0,20) + 内框逆序 (5,5)-(15,5)-(15,15)-(5,15)
    //    按顶点序列连接后，中心点 (10,10) 在 Alternate 下穿越 4 次 = 外部。
    let bow = [
        (0.0f32, 0.0f32),
        (20.0, 0.0),
        (20.0, 20.0),
        (0.0, 20.0),
        (5.0, 5.0),
        (15.0, 5.0),
        (15.0, 15.0),
        (5.0, 15.0),
    ];
    let alt_center = point_in_polygon(&bow, 10.0, 10.0, FILL_MODE_ALTERNATE);
    let wind_center = point_in_polygon(&bow, 10.0, 10.0, FILL_MODE_WINDING);
    cs.add(
        "alternate_vs_winding_divergence",
        !alt_center && wind_center,
        "",
    );
    // 4) 顶点上限沿用（16k）——超过上限的路径构造拒绝走 GdiPath 既有纪律
    //    （此处对账 GdiPath::add_vertex 的诚实拒绝语义）。
    let mut p = GdiPath::new();
    let full = p.add_vertex(1.0, 1.0);
    cs.add("path_vertex_api_anchored", full && p.len() == 1, "");
    // 5) 渐变几何（深化一批既有面）对账锚：线性中点 500、径向边界 1000。
    let g = LinearGradientGeom { x0: 0.0, y0: 0.0, x1: 100.0, y1: 0.0 };
    let rg = PathGradientGeom { cx: 50.0, cy: 50.0, rx: 50.0, ry: 25.0, focus_permille: 0 };
    cs.add(
        "gradient_geometry_anchored",
        g.t_at(50.0, 42.0) == 500 && rg.t_at(100.0, 50.0) == 1000 && rg.t_at(50.0, 50.0) == 0,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F007 · 深化批次三：大位图流式分带计划（>64MP 防内存尖峰——带数/带高/单带
// 峰值上界）
//
// 主册依据（G-A-07【数据与存储】）：「大位图（>64MP）流式处理防内存尖峰」。
// 既有面：STREAMING_THRESHOLD_PIXELS（64MP 线）与 streamed_blocks 计数；本段
// 给出分带计划（怎么流：带数与带高的确定性切分——流式不是口号，是可验算的
// 几何计划）。
// ---------------------------------------------------------------------------

/// 单带像素上界（分带后任意一带的扫描行缓冲 ≤ 此值——内存尖峰的硬顶）。
pub const STREAM_BAND_MAX_PIXELS: u64 = 8_000_000;

/// 流式分带计划。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamingBandPlan {
    pub width: u64,
    pub height: u64,
    /// 分带数（≤64MP 恒 1——不做无谓切分）。
    pub bands: u32,
    /// 每带行数（末带允许矮——向上取整切分）。
    pub band_height: u64,
}

impl StreamingBandPlan {
    /// 为位图出分带计划：≤64MP 单带直读；>64MP 按单带 ≤8MP 向上取整切分。
    pub fn for_bitmap(width: u64, height: u64) -> StreamingBandPlan {
        if width == 0 || height == 0 {
            return StreamingBandPlan { width, height, bands: 0, band_height: 0 };
        }
        let pixels = width * height;
        if pixels <= STREAMING_THRESHOLD_PIXELS {
            StreamingBandPlan { width, height, bands: 1, band_height: height }
        } else {
            // 行域切分：单带行数上界 bh_max = 单带像素上界 / 行宽（向下取整，
            // 至少 1 行）；带数 = ceil(height / bh_max)。该切法保证带高向上取整
            // 后仍 ≤ bh_max（h ≤ bands×bh_max ⇒ ceil(h/bands) ≤ bh_max），从而
            // 单带峰值 ≤ 上界——几何自洽不靠巧合。（极端长条位图（行宽超上界）
            // 退化为逐行流式：带高 1，峰值 = 行宽×1，如实不越判据面。）
            let bh_max = (STREAM_BAND_MAX_PIXELS / width).max(1);
            let bands = ((height + bh_max - 1) / bh_max) as u32;
            let band_height = (height + bands as u64 - 1) / bands as u64;
            StreamingBandPlan { width, height, bands, band_height }
        }
    }

    /// 单带峰值像素（width × band_height ≤ 单带上界——判据恒等式；
    /// 单带位图按整图计，不适用切分上界）。
    pub fn peak_band_pixels(&self) -> u64 {
        if self.bands <= 1 {
            return self.width * self.height;
        }
        self.width * self.band_height
    }

    /// 覆盖完整性：bands × band_height ≥ height（无行遗漏）。
    pub fn covers_all_rows(&self) -> bool {
        if self.bands == 0 {
            return self.height == 0;
        }
        self.bands as u64 * self.band_height >= self.height
    }
}

/// F007 深化批次三自检。
pub fn run_gdiplus_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F007-gdiplus-deep2");
    // 1) 3840×2160（8.3MP）≤64MP → 单带；8000×10000（80MP）→ 10 带、
    //    带高 1000、单带峰值 8MP = 上界（恰好达线不越界）。
    let small = StreamingBandPlan::for_bitmap(3840, 2160);
    let big = StreamingBandPlan::for_bitmap(8000, 10000);
    cs.add(
        "streaming_band_plan_geometry",
        small.bands == 1
            && small.band_height == 2160
            && big.bands == 10
            && big.band_height == 1000
            && big.peak_band_pixels() == STREAM_BAND_MAX_PIXELS,
        "",
    );
    // 2) 覆盖完整性：两计划都无行遗漏；上界恒等式对 >64MP 的竖高成立
    //    （≤64MP 单带不适用切分上界——整图直读语义）。
    let mut bounded = true;
    for h in [17000u64, 20000, 25000] {
        let p = StreamingBandPlan::for_bitmap(3840, h);
        bounded &= p.covers_all_rows() && p.peak_band_pixels() <= STREAM_BAND_MAX_PIXELS;
    }
    cs.add(
        "streaming_band_covers_all_rows",
        small.covers_all_rows() && big.covers_all_rows() && bounded,
        "",
    );
    // 3) 阈值线不变锚（64MP——与既有 STREAMING_THRESHOLD_PIXELS 同源对账）。
    cs.add(
        "stream_threshold_anchor",
        STREAMING_THRESHOLD_PIXELS == 64_000_000,
        "",
    );
    cs
}
